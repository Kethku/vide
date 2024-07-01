use std::sync::Arc;
use wgpu::*;
use winit::{
    event::{Event, StartCause, WindowEvent},
    window::Window,
};

use crate::{drawable::Drawable, Renderer, Scene};

pub struct WinitRenderer<'a> {
    pub instance: Instance,
    pub surface: Option<Surface<'a>>,
    pub surface_config: SurfaceConfiguration,
    window_initializing: bool,
    renderer: Renderer,
    _window: Arc<Window>,
}

impl<'a> WinitRenderer<'a> {
    // Creating some of the wgpu types requires async code
    pub async fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        let surface = instance.create_surface(Arc::clone(&window)).unwrap();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let size = window.inner_size();
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            format: swapchain_format,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::Immediate,
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        let shaders_reloaded = {
            let window = Arc::clone(&window);
            Box::new(move || {
                window.request_redraw();
            })
        };

        let mut renderer = Renderer::new(size.width, size.height, adapter, swapchain_format).await;
        renderer.watch_shaders(shaders_reloaded);
        surface.configure(&renderer.device, &surface_config);

        Self {
            instance,
            window_initializing: false,
            surface: Some(surface),
            surface_config,
            renderer,
            _window: window,
        }
    }

    pub async fn add_drawable<T: Drawable + 'static>(&mut self) {
        self.renderer.add_drawable::<T>().await;
    }

    pub async fn with_drawable<T: Drawable + 'static>(mut self) -> Self {
        self.add_drawable::<T>().await;
        self
    }

    pub async fn add_default_drawables(&mut self) {
        self.renderer.add_default_drawables().await;
    }

    pub async fn with_default_drawables(mut self) -> Self {
        self.add_default_drawables().await;
        self
    }

    fn update_surface(&mut self, surface: Surface<'a>) {
        let swapchain_capabilities = surface.get_capabilities(&self.renderer.adapter);
        let swapchain_format = swapchain_capabilities.formats[0];
        self.surface_config.format = swapchain_format;
        self.surface_config.alpha_mode = swapchain_capabilities.alpha_modes[0];
        surface.configure(&self.renderer.device, &self.surface_config);
        self.surface = Some(surface);
    }

    fn clear_surface(&mut self) {
        self.surface = None;
    }

    fn resize(&mut self, new_width: u32, new_height: u32) {
        self.surface_config.width = new_width;
        self.surface_config.height = new_height;

        if new_width != 0 && new_height != 0 {
            if let Some(surface) = &self.surface {
                surface.configure(&self.renderer.device, &self.surface_config);
            }
            self.renderer.resize(new_width, new_height);
        }
    }

    pub fn handle_event<T>(&mut self, window: &'a Window, event: &Event<T>) {
        match event {
            Event::NewEvents(start_cause) => {
                self.window_initializing = start_cause == &StartCause::Init;
            }
            Event::Resumed => {
                let surface = self.instance.create_surface(window).unwrap();
                self.update_surface(surface);
                window.request_redraw();
            }
            Event::Suspended => {
                self.clear_surface();
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(new_size),
                ..
            } => {
                if self.window_initializing {
                    return;
                }

                self.resize(new_size.width, new_size.height);

                window.request_redraw();
            }
            _ => {}
        }
    }

    pub fn draw(&mut self, scene: &Scene) -> bool {
        let Some(surface) = &mut self.surface else {
            return true;
        };

        match surface.get_current_texture() {
            Ok(frame) => {
                self.renderer.render(scene, &frame.texture);

                {
                    profiling::scope!("present");
                    frame.present();
                }
                profiling::finish_frame!();

                self.renderer.profiler.end_frame().unwrap();

                self.renderer
                    .profiler
                    .process_finished_frame(self.renderer.queue.get_timestamp_period());
                true
            }
            Err(SurfaceError::Lost) => {
                surface.configure(&self.renderer.device, &self.surface_config);
                false
            }
            Err(SurfaceError::OutOfMemory) => false,
            _ => false,
        }
    }
}
