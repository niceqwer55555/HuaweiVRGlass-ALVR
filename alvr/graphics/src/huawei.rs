use super::{ck, staging::create_program, GraphicsContext, GL_TEXTURE_EXTERNAL_OES};
use alvr_common::{glam::UVec2, info};
use glow::{self as gl, HasContext};
use std::{cell::Cell, ffi::c_void, rc::Rc};

/// Renderer for Huawei's legacy OpenXR runtime.
///
/// HVRSDK 3.5.x returns zero from `xrEnumerateSwapchainImages`. Instead, it binds an internal
/// framebuffer from `xrAcquireSwapchainImage`, and its official sample renders directly into that
/// current framebuffer. Keep this path isolated from the standard wgpu swapchain wrapper.
pub struct HuaweiFramebufferRenderer {
    context: Rc<GraphicsContext>,
    view_resolution: UVec2,
    logged_eyes: Cell<u8>,
}

impl HuaweiFramebufferRenderer {
    pub fn new(context: Rc<GraphicsContext>, view_resolution: UVec2) -> Self {
        Self {
            context,
            view_resolution,
            logged_eyes: Cell::new(0),
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
            gl.clear_color(0.02, 0.24, 0.32, 1.0);
            gl.clear(gl::COLOR_BUFFER_BIT);
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

        if hardware_buffer.is_null() {
            gl.clear_color(0.02, 0.24, 0.32, 1.0);
            gl.clear(gl::COLOR_BUFFER_BIT);
        } else {
            self.ensure_resources();
            let resources = self.resources.borrow();
            let resources = resources.as_ref().unwrap();

            self.context.render_ahardwarebuffer_using_texture(
                hardware_buffer,
                resources.surface_texture,
                || unsafe {
                    ck!(gl.use_program(Some(resources.program)));
                    ck!(gl.active_texture(gl::TEXTURE0));
                    ck!(gl.bind_texture(GL_TEXTURE_EXTERNAL_OES, Some(resources.surface_texture)));
                    ck!(gl.bind_sampler(0, None));
                    ck!(gl.uniform_1_i32(Some(&resources.view_idx_uloc), eye as i32));
                    ck!(gl.draw_arrays(gl::TRIANGLE_STRIP, 0, 4));
                },
            );
        }

        gl.flush();

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
