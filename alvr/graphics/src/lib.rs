#[cfg(target_os = "android")]
mod huawei;
#[cfg(target_os = "android")]
mod huawei_lobby;
mod lobby;
mod staging;
mod stream;

#[cfg(target_os = "android")]
pub use huawei::*;
pub use lobby::*;
pub use stream::*;

use alvr_common::{
    glam::{Mat4, UVec2, Vec4},
    Fov,
};
use glow::{self as gl, HasContext};
use khronos_egl as egl;
use std::{ffi::c_void, num::NonZeroU32, ptr};
use wgpu::{
    hal::{self, api, MemoryFlags, TextureUses},
    Adapter, Device, Extent3d, Instance, Queue, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView,
};

#[cfg(target_os = "android")]
#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetCurrentDisplay() -> *mut c_void;
    fn eglGetCurrentContext() -> *mut c_void;
    fn eglGetCurrentSurface(readdraw: i32) -> *mut c_void;
}

#[cfg(target_os = "android")]
#[link(name = "GLESv2")]
unsafe extern "C" {
    fn glBindTexture(target: u32, texture: u32);
    fn glGenTextures(count: i32, textures: *mut u32);
    fn glGetError() -> u32;
    fn glTexImage2D(
        target: u32,
        level: i32,
        internal_format: i32,
        width: i32,
        height: i32,
        border: i32,
        format: u32,
        pixel_type: u32,
        pixels: *const c_void,
    );
    fn glTexParameteri(target: u32, parameter: u32, value: i32);
}

pub const SDR_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
pub const SDR_FORMAT_GL: u32 = gl::RGBA8;
pub const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8D65;
pub const MAX_PUSH_CONSTANTS_SIZE: u32 = 128;

#[cfg(target_os = "android")]
pub struct HuaweiEglContext {
    egl: egl::DynamicInstance<egl::EGL1_4>,
    pub display: egl::Display,
    pub config: egl::Config,
    pub context: egl::Context,
    pub surface: egl::Surface,
}

#[cfg(target_os = "android")]
impl HuaweiEglContext {
    pub fn new(native_window: *mut c_void) -> Self {
        let egl = unsafe {
            egl::DynamicInstance::<egl::EGL1_4>::load_required()
                .expect("failed to load the Android EGL 1.4 API")
        };
        let display = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }
            .expect("failed to acquire the default EGL display");
        let version = egl
            .initialize(display)
            .expect("failed to initialize the default EGL display");

        let mut configs = Vec::with_capacity(
            egl.get_config_count(display)
                .expect("failed to query EGL config count"),
        );
        egl.get_configs(display, &mut configs)
            .expect("failed to enumerate EGL configs");

        // Match Huawei's egl_environment.cpp exactly: first GLES 3 config that supports both
        // window and pbuffer surfaces, with RGB8, depth24 and no multisampling.
        let config = configs
            .into_iter()
            .find(|&config| {
                let attr = |name| egl.get_config_attrib(display, config, name).unwrap_or(-1);
                attr(egl::RENDERABLE_TYPE) & egl::OPENGL_ES3_BIT == egl::OPENGL_ES3_BIT
                    && attr(egl::SURFACE_TYPE) & (egl::WINDOW_BIT | egl::PBUFFER_BIT)
                        == egl::WINDOW_BIT | egl::PBUFFER_BIT
                    && attr(egl::RED_SIZE) == 8
                    && attr(egl::GREEN_SIZE) == 8
                    && attr(egl::BLUE_SIZE) == 8
                    && attr(egl::DEPTH_SIZE) == 24
                    && attr(egl::SAMPLES) == 0
            })
            .expect("Huawei-compatible EGL config not found");

        let context_attributes = [egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE];
        let context = egl
            .create_context(display, config, None, &context_attributes)
            .expect("failed to create the official-style Huawei GLES 3 context");
        let surface = unsafe {
            egl.create_window_surface(display, config, native_window.cast(), None)
                .expect("failed to create the Huawei EGL window surface")
        };
        egl.make_current(display, Some(surface), Some(surface), Some(context))
            .expect("failed to make the Huawei EGL context current");

        let attr = |name| egl.get_config_attrib(display, config, name).unwrap_or(-1);
        alvr_common::info!(
            "[HVR-DIAG] official-first EGL ready: version={}.{}, id={}, rgba={}/{}/{}/{}, depth={}, samples={}, surface=0x{:X}, renderable=0x{:X}, conformant=0x{:X}, display={:p}, config={:p}, context={:p}, window={:p}",
            version.0,
            version.1,
            attr(egl::CONFIG_ID),
            attr(egl::RED_SIZE),
            attr(egl::GREEN_SIZE),
            attr(egl::BLUE_SIZE),
            attr(egl::ALPHA_SIZE),
            attr(egl::DEPTH_SIZE),
            attr(egl::SAMPLES),
            attr(egl::SURFACE_TYPE),
            attr(egl::RENDERABLE_TYPE),
            attr(egl::CONFORMANT),
            display.as_ptr(),
            config.as_ptr(),
            context.as_ptr(),
            native_window,
        );

        Self {
            egl,
            display,
            config,
            context,
            surface,
        }
    }

    pub fn make_current(&self) {
        self.egl
            .make_current(
                self.display,
                Some(self.surface),
                Some(self.surface),
                Some(self.context),
            )
            .expect("failed to restore the Huawei EGL context");
    }

    /// Pushes the rendered frame to the HVR surface. Without this the runtime compositor has
    /// nothing to show and the GL command queue backs up until a synchronous call blocks.
    pub fn swap_buffers(&self) {
        unsafe {
            let _ = self.egl.swap_buffers(self.display, self.surface);
        }
    }

    pub fn get_proc_address(&self, name: &str) -> *const c_void {
        self.egl
            .get_proc_address(name)
            .map_or(ptr::null(), |function| function as *const c_void)
    }

    /// Size of the HVR window surface, i.e. the largest rectangle fbo 0 can cover.
    ///
    /// The surface handed to the client is the phone screen (e.g. 1080x2265), which is *not* the
    /// eye resolution requested by the server. Anything rendered outside this rectangle is clipped
    /// away by the window system, so the direct-framebuffer renderers have to clamp their viewport
    /// to it before copying the result into the eye textures.
    pub fn surface_size(&self) -> (i32, i32) {
        let width = self
            .egl
            .query_surface(self.display, self.surface, egl::WIDTH)
            .unwrap_or(0);
        let height = self
            .egl
            .query_surface(self.display, self.surface, egl::HEIGHT)
            .unwrap_or(0);
        (width, height)
    }
}

type CreateImageFn = unsafe extern "C" fn(
    egl::EGLDisplay,
    egl::EGLContext,
    egl::Enum,
    egl::EGLClientBuffer,
    *const egl::Int,
) -> egl::EGLImage;
type DestroyImageFn = unsafe extern "C" fn(egl::EGLDisplay, egl::EGLImage) -> egl::Boolean;
type GetNativeClientBufferFn = unsafe extern "C" fn(*const c_void) -> egl::EGLClientBuffer;
type ImageTargetTexture2DFn = unsafe extern "C" fn(egl::Enum, egl::EGLImage);

pub(crate) struct ImportedHardwareBuffer {
    image: egl::EGLImage,
}

pub fn check_error(gl: &gl::Context, message_context: &str) {
    let err = unsafe { gl.get_error() };
    if err != glow::NO_ERROR {
        alvr_common::error!("gl error {message_context} -> {err}");
        std::process::abort();
    }
}

macro_rules! ck {
    ($gl_ctx:ident.$($gl_cmd:tt)*) => {{
        let res = $gl_ctx.$($gl_cmd)*;

        #[cfg(debug_assertions)]
        crate::check_error(&$gl_ctx, &format!("{}:{}: {}", file!(), line!(), stringify!($($gl_cmd)*)));

        res
    }};
}
pub(crate) use ck;

fn projection_from_fov(fov: Fov) -> Mat4 {
    const NEAR: f32 = 0.1;

    let tanl = f32::tan(fov.left);
    let tanr = f32::tan(fov.right);
    let tanu = f32::tan(fov.up);
    let tand = f32::tan(fov.down);
    let a = 2.0 / (tanr - tanl);
    let b = 2.0 / (tanu - tand);
    let c = (tanr + tanl) / (tanr - tanl);
    let d = (tanu + tand) / (tanu - tand);

    // note: for wgpu compatibility, the b and d components should be flipped. Maybe a bug in the
    // viewport handling in wgpu?
    Mat4::from_cols(
        Vec4::new(a, 0.0, c, 0.0),
        Vec4::new(0.0, -b, -d, 0.0),
        Vec4::new(0.0, 0.0, -1.0, -NEAR),
        Vec4::new(0.0, 0.0, -1.0, 0.0),
    )
    .transpose()
}

pub fn choose_swapchain_format(supported_formats: &[u32], enable_hdr: bool) -> u32 {
    // Priority-sorted list of swapchain formats we'll accept--
    let mut app_supported_swapchain_formats = vec![gl::SRGB8_ALPHA8, gl::RGBA8];

    // float16 is required for HDR output. However, float16 swapchains
    // have a high perf cost, so only use these if HDR is enabled.
    if enable_hdr {
        app_supported_swapchain_formats.insert(0, gl::RGBA16F);
    }

    for format in app_supported_swapchain_formats {
        if supported_formats.contains(&format) {
            return format;
        }
    }

    // If we can't enumerate, default to a required format
    gl::RGBA8
}

pub fn gl_format_to_wgpu(format: u32) -> TextureFormat {
    match format {
        gl::SRGB8_ALPHA8 => TextureFormat::Rgba8UnormSrgb,
        gl::RGBA8 => TextureFormat::Rgba8Unorm,
        gl::RGBA16F => TextureFormat::Rgba16Float,
        _ => panic!("Unsupported GL format: {format}"),
    }
}

pub fn create_texture(device: &Device, resolution: UVec2, format: TextureFormat) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: resolution.x,
            height: resolution.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_texture_from_gles(
    device: &Device,
    texture: u32,
    resolution: UVec2,
    format: TextureFormat,
) -> Texture {
    let size = Extent3d {
        width: resolution.x,
        height: resolution.y,
        depth_or_array_layers: 1,
    };

    unsafe {
        let hal_texture = device.as_hal::<api::Gles, _, _>(|device| {
            device.unwrap().texture_from_raw(
                NonZeroU32::new(texture).unwrap(),
                &hal::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUses::COLOR_TARGET,
                    memory_flags: MemoryFlags::empty(),
                    view_formats: vec![],
                },
                Some(Box::new(|| ())),
            )
        });

        device.create_texture_from_hal::<api::Gles>(
            hal_texture,
            &TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
        )
    }
}

// This is used to convert OpenXR swapchains to wgpu
pub fn create_gl_swapchain(
    device: &Device,
    gl_textures: &[u32],
    resolution: UVec2,
    format: TextureFormat,
) -> Vec<TextureView> {
    gl_textures
        .iter()
        .map(|gl_tex| {
            create_texture_from_gles(device, *gl_tex, resolution, format)
                .create_view(&Default::default())
        })
        .collect()
}

pub struct GraphicsContext {
    _instance: Instance,

    #[cfg_attr(windows, expect(dead_code))]
    adapter: Adapter,

    device: Device,
    queue: Queue,
    pub egl_display: egl::Display,
    pub egl_config: egl::Config,
    pub egl_context: egl::Context,
    pub gl_context: gl::Context,

    #[cfg(target_os = "android")]
    huawei_egl: Option<HuaweiEglContext>,

    #[cfg_attr(windows, expect(dead_code))]
    render_surface: egl::Surface,

    create_image: CreateImageFn,
    destroy_image: DestroyImageFn,
    get_native_client_buffer: GetNativeClientBufferFn,
    image_target_texture_2d: ImageTargetTexture2DFn,
}

impl GraphicsContext {
    /// Pushes the rendered frame to the HVR surface. Without this the runtime compositor has
    /// nothing to show and the GL command queue backs up until a synchronous call blocks.
    #[cfg(target_os = "android")]
    pub fn swap_huawei_buffers(&self) {
        if let Some(huawei_egl) = &self.huawei_egl {
            huawei_egl.swap_buffers();
        }
    }

    /// Size of the surface fbo 0 points at, or `(0, 0)` when it cannot be queried through EGL
    /// (the non-Huawei paths render into textures and never need this).
    pub fn surface_size(&self) -> (i32, i32) {
        #[cfg(target_os = "android")]
        if let Some(huawei_egl) = &self.huawei_egl {
            return huawei_egl.surface_size();
        }

        (0, 0)
    }

    #[cfg(not(windows))]
    pub fn new_gl() -> Self {
        Self::new_gl_internal(None)
    }

    #[cfg(target_os = "android")]
    pub fn new_gl_with_window(native_window: *mut c_void) -> Self {
        Self::new_gl_internal(Some(native_window))
    }

    #[cfg(target_os = "android")]
    pub fn from_huawei_egl(huawei_egl: HuaweiEglContext) -> Self {
        use std::mem;
        use wgpu::{
            Backends, DeviceDescriptor, Features, InstanceDescriptor, InstanceFlags, Limits,
            MemoryHints,
        };

        const CREATE_IMAGE_FN_STR: &str = "eglCreateImageKHR";
        const DESTROY_IMAGE_FN_STR: &str = "eglDestroyImageKHR";
        const GET_NATIVE_CLIENT_BUFFER_FN_STR: &str = "eglGetNativeClientBufferANDROID";
        const IMAGE_TARGET_TEXTURE_2D_FN_STR: &str = "glEGLImageTargetTexture2DOES";

        // OpenXR has already created and enumerated its images on this context. Only now create
        // wgpu's bookkeeping instance, then restore and adopt the original context as an external
        // GLES adapter. This prevents wgpu from initializing the context seen by Huawei Runtime.
        let flags = if cfg!(debug_assertions) {
            InstanceFlags::DEBUG | InstanceFlags::VALIDATION
        } else {
            InstanceFlags::empty()
        };
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::GL,
            flags,
            backend_options: Default::default(),
        });
        huawei_egl.make_current();

        let exposed_adapter = unsafe {
            <api::Gles as hal::Api>::Adapter::new_external(|name| huawei_egl.get_proc_address(name))
        }
        .expect("failed to adopt the Huawei GLES context in wgpu-hal");
        let adapter = unsafe { instance.create_adapter_from_hal(exposed_adapter) };
        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                label: Some("Huawei VR external GLES device"),
                required_features: Features::PUSH_CONSTANTS,
                required_limits: Limits {
                    max_push_constant_size: MAX_PUSH_CONSTANTS_SIZE,
                    ..adapter.limits()
                },
                memory_hints: MemoryHints::Performance,
            },
            None,
        ))
        .expect("failed to create a wgpu device from the Huawei GLES context");
        huawei_egl.make_current();

        let gl_context =
            unsafe { gl::Context::from_loader_function(|name| huawei_egl.get_proc_address(name)) };
        unsafe {
            alvr_common::info!(
                "[HVR-DIAG] wgpu adopted official-first GLES context: adapter={:?}, version={}, vendor={}, renderer={}",
                adapter.get_info(),
                gl_context.get_parameter_string(gl::VERSION),
                gl_context.get_parameter_string(gl::VENDOR),
                gl_context.get_parameter_string(gl::RENDERER),
            );
        }

        let get_fn_ptr = |name| huawei_egl.get_proc_address(name);
        let create_image: CreateImageFn =
            unsafe { mem::transmute(get_fn_ptr(CREATE_IMAGE_FN_STR)) };
        let destroy_image: DestroyImageFn =
            unsafe { mem::transmute(get_fn_ptr(DESTROY_IMAGE_FN_STR)) };
        let get_native_client_buffer: GetNativeClientBufferFn =
            unsafe { mem::transmute(get_fn_ptr(GET_NATIVE_CLIENT_BUFFER_FN_STR)) };
        let image_target_texture_2d: ImageTargetTexture2DFn =
            unsafe { mem::transmute(get_fn_ptr(IMAGE_TARGET_TEXTURE_2D_FN_STR)) };

        Self {
            _instance: instance,
            adapter,
            device,
            queue,
            egl_display: huawei_egl.display,
            egl_config: huawei_egl.config,
            egl_context: huawei_egl.context,
            gl_context,
            render_surface: huawei_egl.surface,
            huawei_egl: Some(huawei_egl),
            create_image,
            destroy_image,
            get_native_client_buffer,
            image_target_texture_2d,
        }
    }

    #[cfg(not(windows))]
    fn new_gl_internal(native_window: Option<*mut c_void>) -> Self {
        use std::mem;
        use wgpu::{
            Backends, DeviceDescriptor, Features, InstanceDescriptor, InstanceFlags, Limits,
            MemoryHints,
        };

        const CREATE_IMAGE_FN_STR: &str = "eglCreateImageKHR";
        const DESTROY_IMAGE_FN_STR: &str = "eglDestroyImageKHR";
        const GET_NATIVE_CLIENT_BUFFER_FN_STR: &str = "eglGetNativeClientBufferANDROID";
        const IMAGE_TARGET_TEXTURE_2D_FN_STR: &str = "glEGLImageTargetTexture2DOES";

        let flags = if cfg!(debug_assertions) {
            InstanceFlags::DEBUG | InstanceFlags::VALIDATION
        } else {
            InstanceFlags::empty()
        };

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::GL,
            flags,
            backend_options: Default::default(),
        });

        let adapter = instance.enumerate_adapters(Backends::GL).remove(0);
        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                label: None,
                required_features: Features::PUSH_CONSTANTS,
                required_limits: Limits {
                    max_push_constant_size: MAX_PUSH_CONSTANTS_SIZE,
                    ..adapter.limits()
                },
                memory_hints: MemoryHints::Performance,
            },
            None,
        ))
        .unwrap();

        let raw_instance = unsafe { instance.as_hal::<api::Gles>() }.unwrap();

        let egl_display = raw_instance.raw_display();
        let egl_config = raw_instance.egl_config();

        let (
            egl_context,
            gl_context,
            render_surface,
            create_image,
            destroy_image,
            get_native_client_buffer,
            image_target_texture_2d,
        ) = unsafe {
            adapter.as_hal::<api::Gles, _, _>(|raw_adapter| {
                let adapter_context = raw_adapter.unwrap().adapter_context();
                let egl_instance = adapter_context.egl_instance().unwrap();

                let egl_context = egl::Context::from_ptr(adapter_context.raw_context());

                if native_window.is_some() {
                    let config_attr = |attribute| {
                        egl_instance
                            .get_config_attrib(egl_display, egl_config, attribute)
                            .unwrap_or(-1)
                    };
                    alvr_common::info!(
                        "[HVR-DIAG] selected EGL config: id={}, rgba={}/{}/{}/{}, depth={}, samples={}, surface=0x{:X}, renderable=0x{:X}, conformant=0x{:X}, native={}",
                        config_attr(egl::CONFIG_ID),
                        config_attr(egl::RED_SIZE),
                        config_attr(egl::GREEN_SIZE),
                        config_attr(egl::BLUE_SIZE),
                        config_attr(egl::ALPHA_SIZE),
                        config_attr(egl::DEPTH_SIZE),
                        config_attr(egl::SAMPLES),
                        config_attr(egl::SURFACE_TYPE),
                        config_attr(egl::RENDERABLE_TYPE),
                        config_attr(egl::CONFORMANT),
                        config_attr(egl::NATIVE_RENDERABLE),
                    );
                }

                let render_surface = if let Some(native_window) = native_window {
                    // Huawei's runtime only exports valid GLES swapchain textures when the
                    // session context is current on the Activity's real window surface.
                    egl_instance
                        .create_window_surface(egl_display, egl_config, native_window.cast(), None)
                        .unwrap()
                } else {
                    const PBUFFER_ATTRIBS: [i32; 5] = [egl::WIDTH, 16, egl::HEIGHT, 16, egl::NONE];
                    egl_instance
                        .create_pbuffer_surface(egl_display, egl_config, &PBUFFER_ATTRIBS)
                        .unwrap()
                };

                egl_instance
                    .make_current(
                        egl_display,
                        Some(render_surface),
                        Some(render_surface),
                        Some(egl_context),
                    )
                    .unwrap();

                let gl_context = gl::Context::from_loader_function(|fn_name| {
                    egl_instance
                        .get_proc_address(fn_name)
                        .map_or(ptr::null(), |f| f as *const c_void)
                });

                if native_window.is_some() {
                    alvr_common::info!(
                        "[HVR-DIAG] GLES context: version={}, vendor={}, renderer={}",
                        gl_context.get_parameter_string(gl::VERSION),
                        gl_context.get_parameter_string(gl::VENDOR),
                        gl_context.get_parameter_string(gl::RENDERER),
                    );
                }

                let get_fn_ptr = |fn_name| {
                    egl_instance
                        .get_proc_address(fn_name)
                        .map_or(ptr::null(), |f| f as *const c_void)
                };

                let create_image: CreateImageFn = mem::transmute(get_fn_ptr(CREATE_IMAGE_FN_STR));
                let destroy_image: DestroyImageFn =
                    mem::transmute(get_fn_ptr(DESTROY_IMAGE_FN_STR));
                let get_native_client_buffer: GetNativeClientBufferFn =
                    mem::transmute(get_fn_ptr(GET_NATIVE_CLIENT_BUFFER_FN_STR));
                let image_target_texture_2d: ImageTargetTexture2DFn =
                    mem::transmute(get_fn_ptr(IMAGE_TARGET_TEXTURE_2D_FN_STR));

                (
                    egl_context,
                    gl_context,
                    render_surface,
                    create_image,
                    destroy_image,
                    get_native_client_buffer,
                    image_target_texture_2d,
                )
            })
        };

        Self {
            _instance: instance,
            adapter,
            device,
            queue,
            egl_display,
            egl_config,
            egl_context,
            gl_context,
            #[cfg(target_os = "android")]
            huawei_egl: None,
            render_surface,
            create_image,
            destroy_image,
            get_native_client_buffer,
            image_target_texture_2d,
        }
    }

    #[cfg(windows)]
    pub fn new_gl() -> Self {
        unimplemented!()
    }

    pub fn make_current(&self) {
        #[cfg(target_os = "android")]
        if let Some(huawei_egl) = &self.huawei_egl {
            huawei_egl.make_current();
            return;
        }

        #[cfg(not(windows))]
        unsafe {
            self.adapter.as_hal::<api::Gles, _, _>(|raw_adapter| {
                let egl_instance = raw_adapter
                    .unwrap()
                    .adapter_context()
                    .egl_instance()
                    .unwrap();

                egl_instance
                    .make_current(
                        self.egl_display,
                        Some(self.render_surface),
                        Some(self.render_surface),
                        Some(self.egl_context),
                    )
                    .unwrap();
            })
        };
    }

    #[cfg(not(windows))]
    pub fn log_current_egl(&self, stage: &str) {
        #[cfg(target_os = "android")]
        if self.huawei_egl.is_some() {
            unsafe {
                alvr_common::info!(
                    "[HVR-DIAG] official-first EGL current at {stage}: display={:?}, context={:?}, draw={:?}, read={:?}, gl_error=0x{:X}",
                    eglGetCurrentDisplay(),
                    eglGetCurrentContext(),
                    eglGetCurrentSurface(egl::DRAW),
                    eglGetCurrentSurface(egl::READ),
                    glGetError(),
                );
            }
            return;
        }

        unsafe {
            self.adapter.as_hal::<api::Gles, _, _>(|raw_adapter| {
                let egl_instance = raw_adapter
                    .unwrap()
                    .adapter_context()
                    .egl_instance()
                    .unwrap();
                let current_display = egl_instance.get_current_display().map(|value| value.as_ptr());
                let current_context = egl_instance.get_current_context().map(|value| value.as_ptr());
                let draw_surface = egl_instance
                    .get_current_surface(egl::DRAW)
                    .map(|value| value.as_ptr());
                let read_surface = egl_instance
                    .get_current_surface(egl::READ)
                    .map(|value| value.as_ptr());

                alvr_common::info!(
                    "[HVR-DIAG] EGL current at {stage}: display={current_display:?}, context={current_context:?}, draw={draw_surface:?}, read={read_surface:?}, gl_error=0x{:X}",
                    self.gl_context.get_error()
                );

                #[cfg(target_os = "android")]
                alvr_common::info!(
                    "[HVR-DIAG] directly linked EGL/GLES at {stage}: display={:?}, context={:?}, draw={:?}, read={:?}, gl_error=0x{:X}",
                    eglGetCurrentDisplay(),
                    eglGetCurrentContext(),
                    eglGetCurrentSurface(egl::DRAW),
                    eglGetCurrentSurface(egl::READ),
                    glGetError(),
                );
            });
        }
    }

    #[cfg(target_os = "android")]
    pub fn warm_up_huawei_gles(&self) -> u32 {
        const GL_LINEAR: i32 = 0x2601;
        const GL_RGBA: u32 = 0x1908;
        const GL_TEXTURE_2D: u32 = 0x0DE1;
        const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
        const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
        const GL_UNSIGNED_BYTE: u32 = 0x1401;

        unsafe {
            while glGetError() != gl::NO_ERROR {}

            let mut texture = 0;
            glGenTextures(1, &mut texture);
            glBindTexture(GL_TEXTURE_2D, texture);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                1,
                1,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                ptr::null(),
            );
            glBindTexture(GL_TEXTURE_2D, 0);

            alvr_common::info!(
                "[HVR-DIAG] direct GLES warm-up texture before xrCreateSession: texture={texture}, gl_error=0x{:X}",
                glGetError()
            );

            texture
        }
    }

    /// # Safety
    /// `buffer` must be a valid AHardwareBuffer.
    /// `texture` must be a valid GL texture.
    pub(crate) unsafe fn import_ahardwarebuffer_to_texture(
        &self,
        buffer: *const c_void,
        texture: gl::Texture,
    ) -> Option<ImportedHardwareBuffer> {
        const EGL_NATIVE_BUFFER_ANDROID: u32 = 0x3140;

        if buffer.is_null() {
            return None;
        }

        let client_buffer = (self.get_native_client_buffer)(buffer);
        check_error(&self.gl_context, "get_native_client_buffer");

        let image = (self.create_image)(
            self.egl_display.as_ptr(),
            egl::NO_CONTEXT,
            EGL_NATIVE_BUFFER_ANDROID,
            client_buffer,
            ptr::null(),
        );
        check_error(&self.gl_context, "create_image");

        self.gl_context
            .bind_texture(GL_TEXTURE_EXTERNAL_OES, Some(texture));
        check_error(&self.gl_context, "bind texture OES");

        (self.image_target_texture_2d)(GL_TEXTURE_EXTERNAL_OES, image);
        check_error(&self.gl_context, "image_target_texture_2d");

        Some(ImportedHardwareBuffer { image })
    }

    /// # Safety
    /// `imported_buffer` must have been created by this graphics context. Rendering commands that
    /// reference its texture must have been submitted before release.
    pub(crate) unsafe fn release_imported_hardware_buffer(
        &self,
        imported_buffer: ImportedHardwareBuffer,
    ) {
        (self.destroy_image)(self.egl_display.as_ptr(), imported_buffer.image);
        check_error(&self.gl_context, "destroy_image");
    }

    /// # Safety
    /// `buffer` must be a valid AHardwareBuffer.
    /// `texture` must be a valid GL texture.
    pub unsafe fn render_ahardwarebuffer_using_texture(
        &self,
        buffer: *const c_void,
        texture: gl::Texture,
        render_cb: impl FnOnce(),
    ) {
        if let Some(imported_buffer) = self.import_ahardwarebuffer_to_texture(buffer, texture) {
            render_cb();
            self.release_imported_hardware_buffer(imported_buffer);
        }
    }
}

#[cfg(not(windows))]
impl Default for GraphicsContext {
    fn default() -> Self {
        Self::new_gl()
    }
}
