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
    rc::Rc,
};

struct HuaweiLobbyResources {
    program: gl::Program,
    texture: gl::Texture,
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
}

impl HuaweiFramebufferRenderer {
    pub fn new(
        context: Rc<GraphicsContext>,
        view_resolution: UVec2,
        initial_hud_message: &str,
    ) -> Self {
        Self {
            context,
            view_resolution,
            resources: RefCell::new(None),
            hud_message: RefCell::new(initial_hud_message.to_owned()),
            hud_dirty: Cell::new(true),
            logged_eyes: Cell::new(0),
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
        let gl = &self.context.gl_context;

        unsafe {
            let framebuffer = gl.get_parameter_i32(gl::FRAMEBUFFER_BINDING);
            let framebuffer_status = gl.check_framebuffer_status(gl::FRAMEBUFFER);

            gl.disable(gl::SCISSOR_TEST);
            gl.viewport(
                0,
                0,
                self.view_resolution.x as i32,
                self.view_resolution.y as i32,
            );
            gl.color_mask(true, true, true, true);
            gl.clear_color(0.027, 0.078, 0.118, 1.0);
            gl.clear(gl::COLOR_BUFFER_BIT);

            self.ensure_resources();
            self.update_texture_if_needed();
            let resources = self.resources.borrow();
            let resources = resources.as_ref().unwrap();
            gl.use_program(Some(resources.program));
            gl.active_texture(gl::TEXTURE0);
            gl.bind_texture(gl::TEXTURE_2D, Some(resources.texture));
            gl.bind_sampler(0, None);
            gl.disable(gl::DEPTH_TEST);
            gl.disable(gl::CULL_FACE);
            gl.disable(gl::BLEND);
            gl.draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
            gl.flush();

            let gl_error = gl.get_error();
            let eye_bit = 1_u8 << eye;
            if self.logged_eyes.get() & eye_bit == 0 {
                info!(
                    "[HVR-DIAG] Huawei direct framebuffer render: eye={eye}, fbo={framebuffer}, status=0x{framebuffer_status:X}, gl_error=0x{gl_error:X}"
                );
                self.logged_eyes.set(self.logged_eyes.get() | eye_bit);
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
}

impl HuaweiStreamRenderer {
    pub fn new(
        context: Rc<GraphicsContext>,
        view_resolution: UVec2,
        fix_limited_range: bool,
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
        let framebuffer = gl.get_parameter_i32(gl::FRAMEBUFFER_BINDING);
        let framebuffer_status = gl.check_framebuffer_status(gl::FRAMEBUFFER);

        if self.gl_ready_eyes.get() & eye_bit == 0 {
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
        gl.viewport(
            0,
            0,
            self.view_resolution.x as i32,
            self.view_resolution.y as i32,
        );
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

        if eye == 1 {
            if let Some(imported_frame) = self.imported_frame.borrow_mut().take() {
                self.context
                    .release_imported_hardware_buffer(imported_frame);
            }
        }

        let gl_error = gl.get_error();
        if self.logged_eyes.get() & eye_bit == 0 {
            info!(
                "[HVR-DIAG] Huawei direct stream render: eye={eye}, decoded={}, fbo={framebuffer}, status=0x{framebuffer_status:X}, gl_error=0x{gl_error:X}",
                !hardware_buffer.is_null()
            );
            self.logged_eyes.set(self.logged_eyes.get() | eye_bit);
        }
    }
}
