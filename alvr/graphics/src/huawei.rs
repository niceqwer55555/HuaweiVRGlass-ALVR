use super::{
    ck,
    huawei_lobby::{render_huawei_lobby_texture, HUAWEI_LOBBY_TEXTURE_SIDE},
    staging::create_program,
    GraphicsContext, ImportedHardwareBuffer, GL_TEXTURE_EXTERNAL_OES,
};
use alvr_common::{glam::UVec2, info};
use glow::{self as gl, HasContext};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    num::NonZeroU32,
    rc::Rc,
};

struct HuaweiLobbyResources {
    program: gl::Program,
    texture: gl::Texture,
}

/// Copy the square region `(0, 0, src_side, src_side)` of the default framebuffer into an eye
/// texture.
///
/// This is the piece the OpenXR runtime would normally do for us: Huawei's runtime binds the eye's
/// framebuffer as fbo 0 at `xrAcquireSwapchainImage` time and copies it out on release. Our shim
/// advertises real GL textures instead, but the renderers below still draw into fbo 0, so the
/// content has to be pulled out by hand.
///
/// Two details matter:
///   * it must run **before** `swap_huawei_buffers()` — a window surface back buffer is undefined
///     after `eglSwapBuffers`, so anything copied afterwards is garbage;
///   * both eyes cover the same rectangle, so eye 0's result is destroyed by eye 1's clear/draw.
///     Copying right after each eye's draw call is what keeps the eyes apart.
///
/// FBOs are not shared between EGL contexts, so the scratch framebuffer is created lazily in
/// whatever context is current and cached per renderer.
unsafe fn blit_default_fbo_to_eye_texture(
    gl: &gl::Context,
    scratch_fbo: &Cell<Option<gl::Framebuffer>>,
    dst_tex: gl::Texture,
    dst_size: UVec2,
    src_side: i32,
) -> bool {
    if src_side <= 0 || dst_size.x == 0 || dst_size.y == 0 {
        return false;
    }

    let framebuffer = match scratch_fbo.get() {
        Some(framebuffer) => framebuffer,
        None => match gl.create_framebuffer() {
            Ok(framebuffer) => {
                scratch_fbo.set(Some(framebuffer));
                framebuffer
            }
            Err(_) => return false,
        },
    };

    gl.bind_framebuffer(gl::DRAW_FRAMEBUFFER, Some(framebuffer));
    gl.framebuffer_texture_2d(
        gl::DRAW_FRAMEBUFFER,
        gl::COLOR_ATTACHMENT0,
        gl::TEXTURE_2D,
        Some(dst_tex),
        0,
    );
    let complete = gl.check_framebuffer_status(gl::DRAW_FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE;
    gl.bind_framebuffer(gl::READ_FRAMEBUFFER, None);
    if complete {
        gl.blit_framebuffer(
            0,
            0,
            src_side,
            src_side,
            0,
            0,
            dst_size.x as i32,
            dst_size.y as i32,
            gl::COLOR_BUFFER_BIT,
            gl::LINEAR,
        );
    }
    gl.bind_framebuffer(gl::FRAMEBUFFER, None);
    gl.flush();

    complete
}

/// The largest square that fits both the requested eye resolution and the fbo 0 surface.
///
/// The server may ask for e.g. 1600x1600 while the HVR window surface is only 1080x2265 wide; a
/// viewport wider than the surface is silently clipped, which would leave part of the eye texture
/// undefined.
fn clamp_side(context: &GraphicsContext, view_resolution: UVec2) -> i32 {
    let mut side = view_resolution.x.min(view_resolution.y) as i32;
    let (surface_width, surface_height) = context.surface_size();
    if surface_width > 0 && surface_height > 0 {
        side = side.min(surface_width).min(surface_height);
    }
    side.max(1)
}

/// Renderer for Huawei's legacy OpenXR runtime.
///
/// HVRSDK 3.5.x returns zero from `xrEnumerateSwapchainImages`. Instead, it binds an internal
/// framebuffer from `xrAcquireSwapchainImage`, and its official sample renders directly into that
/// current framebuffer. Keep this path isolated from the standard wgpu swapchain wrapper.
pub struct HuaweiFramebufferRenderer {
    context: Rc<GraphicsContext>,
    view_resolution: UVec2,
    resources: RefCell<Option<HuaweiLobbyResources>>,
    hud_message: RefCell<String>,
    hud_dirty: Cell<bool>,
    logged_eyes: Cell<u8>,
    eye_textures: [Vec<u32>; 2],
    scratch_fbos: [Cell<Option<gl::Framebuffer>>; 2],
    logged_capture: Cell<u8>,
}

impl HuaweiFramebufferRenderer {
    pub fn new(
        context: Rc<GraphicsContext>,
        view_resolution: UVec2,
        initial_hud_message: &str,
        eye_textures: [Vec<u32>; 2],
    ) -> Self {
        Self {
            context,
            view_resolution,
            resources: RefCell::new(None),
            hud_message: RefCell::new(initial_hud_message.to_owned()),
            hud_dirty: Cell::new(true),
            logged_eyes: Cell::new(0),
            eye_textures,
            scratch_fbos: [Cell::new(None), Cell::new(None)],
            logged_capture: Cell::new(0),
        }
    }

    pub fn update_hud_message(&self, message: &str) {
        let mut current_message = self.hud_message.borrow_mut();
        if current_message.as_str() != message {
            current_message.clear();
            current_message.push_str(message);
            self.hud_dirty.set(true);
        }
    }

    fn ensure_resources(&self) {
        if self.resources.borrow().is_some() {
            return;
        }

        let gl = &self.context.gl_context;
        let program = create_program(
            gl,
            include_str!("../resources/huawei_lobby_vertex.glsl"),
            include_str!("../resources/huawei_lobby_fragment.glsl"),
        );

        unsafe {
            let texture = ck!(gl.create_texture().unwrap());
            ck!(gl.active_texture(gl::TEXTURE0));
            ck!(gl.bind_texture(gl::TEXTURE_2D, Some(texture)));
            ck!(gl.tex_parameter_i32(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32));
            ck!(gl.tex_parameter_i32(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32));
            ck!(gl.tex_parameter_i32(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32));
            ck!(gl.tex_parameter_i32(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32));
            ck!(gl.use_program(Some(program)));
            let sampler = ck!(gl.get_uniform_location(program, "lobby_texture")).unwrap();
            ck!(gl.uniform_1_i32(Some(&sampler), 0));

            self.resources
                .replace(Some(HuaweiLobbyResources { program, texture }));
        }
    }

    fn update_texture_if_needed(&self) {
        if !self.hud_dirty.replace(false) {
            return;
        }

        let pixels = render_huawei_lobby_texture(&self.hud_message.borrow());
        let gl = &self.context.gl_context;
        let resources = self.resources.borrow();
        let resources = resources.as_ref().unwrap();

        unsafe {
            ck!(gl.active_texture(gl::TEXTURE0));
            ck!(gl.bind_texture(gl::TEXTURE_2D, Some(resources.texture)));
            ck!(gl.pixel_store_i32(gl::UNPACK_ALIGNMENT, 1));
            ck!(gl.tex_image_2d(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                HUAWEI_LOBBY_TEXTURE_SIDE as i32,
                HUAWEI_LOBBY_TEXTURE_SIDE as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                gl::PixelUnpackData::Slice(Some(&pixels)),
            ));
        }
    }

    pub fn render_view(&self, eye: usize) {
        static RV_TRACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let trace = RV_TRACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 140;

        let gl = &self.context.gl_context;

        if trace {
            info!("[HVR-DIAG] rv eye {eye}: start");
        }

        unsafe {
            gl.disable(gl::SCISSOR_TEST);
            let side = clamp_side(&self.context, self.view_resolution);
            gl.viewport(0, 0, side, side);
            gl.color_mask(true, true, true, true);
            gl.clear_color(0.027, 0.078, 0.118, 1.0);
            gl.clear(gl::COLOR_BUFFER_BIT);

            if trace {
                info!("[HVR-DIAG] rv eye {eye}: cleared");
            }

            self.ensure_resources();

            if trace {
                info!("[HVR-DIAG] rv eye {eye}: resources ok");
            }

            let hud_dirty = self.hud_dirty.get();
            self.update_texture_if_needed();

            if trace {
                info!("[HVR-DIAG] rv eye {eye}: texture ok (dirty={hud_dirty})");
            }

            let resources = self.resources.borrow();
            let resources = resources.as_ref().unwrap();
            gl.use_program(Some(resources.program));
            gl.active_texture(gl::TEXTURE0);
            gl.bind_texture(gl::TEXTURE_2D, Some(resources.texture));
            gl.bind_sampler(0, None);
            gl.disable(gl::DEPTH_TEST);
            gl.disable(gl::CULL_FACE);
            gl.disable(gl::BLEND);

            if trace {
                info!("[HVR-DIAG] rv eye {eye}: state set");
            }

            gl.draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
            gl.flush();

            if trace {
                info!("[HVR-DIAG] rv eye {eye}: drawn");
            }

            let eye_bit = 1_u8 << eye;

            // Hand this eye to the runtime while it is still on screen: the other eye draws over the
            // same rectangle and the swap below invalidates the back buffer.
            if let Some(texture) = self.eye_textures[eye]
                .first()
                .and_then(|id| NonZeroU32::new(*id))
                .map(gl::NativeTexture)
            {
                let captured = blit_default_fbo_to_eye_texture(
                    gl,
                    &self.scratch_fbos[eye],
                    texture,
                    self.view_resolution,
                    side,
                );
                if self.logged_capture.get() & eye_bit == 0 {
                    info!(
                        "[HVR-DIAG] lobby eye {eye}: {}x{side} fbo0 -> eye texture {}x{} (side={side}), ok={captured}",
                        side, self.view_resolution.x, self.view_resolution.y
                    );
                    self.logged_capture.set(self.logged_capture.get() | eye_bit);
                }
            }
            if self.logged_eyes.get() & eye_bit == 0 {
                // These queries are synchronous and block as soon as the GPU queue backs up, so
                // only ask once per eye.
                let framebuffer = gl.get_parameter_i32(gl::FRAMEBUFFER_BINDING);
                let framebuffer_status = gl.check_framebuffer_status(gl::FRAMEBUFFER);
                let gl_error = gl.get_error();
                info!(
                    "[HVR-DIAG] Huawei direct framebuffer render: eye={eye}, fbo={framebuffer}, status=0x{framebuffer_status:X}, gl_error=0x{gl_error:X}"
                );
                self.logged_eyes.set(self.logged_eyes.get() | eye_bit);
            }

            if eye == 1 {
                self.context.swap_huawei_buffers();
                if trace {
                    info!("[HVR-DIAG] rv eye {eye}: swapped");
                }
            }
        }
    }
}

struct HuaweiStreamResources {
    program: gl::Program,
    view_idx_uloc: gl::UniformLocation,
    surface_texture: gl::Texture,
}

/// Stream renderer for Huawei's legacy framebuffer-on-acquire path.
///
/// GL resources are created lazily after `xrAcquireSwapchainImage` because that is when HVRSDK
/// makes the application context current and binds the eye's internal framebuffer. In particular,
/// `new()` must not attempt to make the graphics context current while the runtime owns it.
pub struct HuaweiStreamRenderer {
    context: Rc<GraphicsContext>,
    view_resolution: UVec2,
    fix_limited_range: bool,
    resources: std::cell::RefCell<Option<HuaweiStreamResources>>,
    imported_frame: std::cell::RefCell<Option<ImportedHardwareBuffer>>,
    entered_eyes: Cell<u8>,
    gl_ready_eyes: Cell<u8>,
    logged_eyes: Cell<u8>,
    eye_textures: [Vec<u32>; 2],
    scratch_fbos: [Cell<Option<gl::Framebuffer>>; 2],
    logged_capture: Cell<u8>,
}

impl HuaweiStreamRenderer {
    pub fn new(
        context: Rc<GraphicsContext>,
        view_resolution: UVec2,
        fix_limited_range: bool,
        eye_textures: [Vec<u32>; 2],
    ) -> Self {
        Self {
            context,
            view_resolution,
            fix_limited_range,
            resources: std::cell::RefCell::new(None),
            imported_frame: std::cell::RefCell::new(None),
            entered_eyes: Cell::new(0),
            gl_ready_eyes: Cell::new(0),
            logged_eyes: Cell::new(0),
            eye_textures,
            scratch_fbos: [Cell::new(None), Cell::new(None)],
            logged_capture: Cell::new(0),
        }
    }

    fn ensure_resources(&self) {
        if self.resources.borrow().is_some() {
            return;
        }

        let gl = &self.context.gl_context;

        let mut fragment_lines: Vec<&str> = include_str!("../resources/staging_fragment.glsl")
            .lines()
            .collect();
        if self.fix_limited_range {
            fragment_lines.insert(1, "#line 0 1\n#define FIX_LIMITED_RANGE");
        }

        let program = create_program(
            gl,
            include_str!("../resources/huawei_stream_vertex.glsl"),
            &fragment_lines.join("\n"),
        );

        unsafe {
            let surface_texture = ck!(gl.create_texture().unwrap());
            let view_idx_uloc = ck!(gl.get_uniform_location(program, "view_idx")).unwrap();

            self.resources.replace(Some(HuaweiStreamResources {
                program,
                view_idx_uloc,
                surface_texture,
            }));
        }
    }

    /// # Safety
    /// `hardware_buffer` must be null or point to a valid `AHardwareBuffer` owned by the decoder.
    /// The caller must invoke this only after acquiring and waiting for the corresponding eye.
    pub unsafe fn render_view(&self, hardware_buffer: *mut c_void, eye: usize) {
        let eye_bit = 1_u8 << eye;
        if self.entered_eyes.get() & eye_bit == 0 {
            info!(
                "[HVR-DIAG] Huawei direct stream render called: eye={eye}, decoded={}",
                !hardware_buffer.is_null()
            );
            self.entered_eyes.set(self.entered_eyes.get() | eye_bit);
        }

        let gl = &self.context.gl_context;

        if self.gl_ready_eyes.get() & eye_bit == 0 {
            let framebuffer = gl.get_parameter_i32(gl::FRAMEBUFFER_BINDING);
            let framebuffer_status = gl.check_framebuffer_status(gl::FRAMEBUFFER);
            info!(
                "[HVR-DIAG] Huawei direct stream GL state ready: eye={eye}, fbo={framebuffer}, status=0x{framebuffer_status:X}"
            );
            self.gl_ready_eyes.set(self.gl_ready_eyes.get() | eye_bit);
        }

        gl.disable(gl::SCISSOR_TEST);
        gl.disable(gl::STENCIL_TEST);
        gl.disable(gl::DEPTH_TEST);
        gl.disable(gl::CULL_FACE);
        gl.disable(gl::BLEND);
        let side = clamp_side(&self.context, self.view_resolution);
        gl.viewport(0, 0, side, side);
        gl.color_mask(true, true, true, true);

        if eye == 0 {
            if let Some(stale_import) = self.imported_frame.borrow_mut().take() {
                self.context.release_imported_hardware_buffer(stale_import);
            }
        }

        if hardware_buffer.is_null() {
            gl.clear_color(0.02, 0.24, 0.32, 1.0);
            gl.clear(gl::COLOR_BUFFER_BIT);
        } else {
            self.ensure_resources();
            let resources = self.resources.borrow();
            let resources = resources.as_ref().unwrap();

            if self.imported_frame.borrow().is_none() {
                self.imported_frame.replace(
                    self.context.import_ahardwarebuffer_to_texture(
                        hardware_buffer,
                        resources.surface_texture,
                    ),
                );
            }

            ck!(gl.use_program(Some(resources.program)));
            ck!(gl.active_texture(gl::TEXTURE0));
            ck!(gl.bind_texture(GL_TEXTURE_EXTERNAL_OES, Some(resources.surface_texture)));
            ck!(gl.bind_sampler(0, None));
            ck!(gl.uniform_1_i32(Some(&resources.view_idx_uloc), eye as i32));
            ck!(gl.draw_arrays(gl::TRIANGLE_STRIP, 0, 4));
        }

        gl.flush();

        // Hand this eye to the runtime before the second eye paints over the same rectangle and
        // before `swap_huawei_buffers()` throws the back buffer away.
        if let Some(texture) = self.eye_textures[eye]
            .first()
            .and_then(|id| NonZeroU32::new(*id))
            .map(gl::NativeTexture)
        {
            let captured = blit_default_fbo_to_eye_texture(
                gl,
                &self.scratch_fbos[eye],
                texture,
                self.view_resolution,
                side,
            );
            if self.logged_capture.get() & eye_bit == 0 {
                info!(
                    "[HVR-DIAG] stream eye {eye}: {side}x{side} fbo0 -> eye texture {}x{} ok={captured}",
                    self.view_resolution.x, self.view_resolution.y
                );
                self.logged_capture.set(self.logged_capture.get() | eye_bit);
            }
        }

        if eye == 1 {
            if let Some(imported_frame) = self.imported_frame.borrow_mut().take() {
                self.context
                    .release_imported_hardware_buffer(imported_frame);
            }

            self.context.swap_huawei_buffers();
        }

        if self.logged_eyes.get() & eye_bit == 0 {
            // Synchronous queries block once the GPU queue backs up, so only ask once per eye.
            let gl_error = gl.get_error();
            info!(
                "[HVR-DIAG] Huawei direct stream render: eye={eye}, decoded={}, gl_error=0x{gl_error:X}",
                !hardware_buffer.is_null()
            );
            self.logged_eyes.set(self.logged_eyes.get() | eye_bit);
        }
    }
}
