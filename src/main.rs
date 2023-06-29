use egui_wgpu::renderer::ScreenDescriptor;
use egui_wgpu::Renderer;
use egui_winit::State;
use wgpu::InstanceDescriptor;
use winit::event::Event::*;
use winit::event_loop::ControlFlow;

use std::iter;
use std::time::Instant;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

/// A simple egui + wgpu + winit based example.
fn main() {
    let event_loop = winit::event_loop::EventLoopBuilder::<()>::with_user_event().build();
    let mut window = winit::window::WindowBuilder::new().with_title("egui-wgpu-winit example");

    window = window.with_inner_size(winit::dpi::PhysicalSize {
        width: INITIAL_WIDTH,
        height: INITIAL_HEIGHT,
    });

    let window = window.build(&event_loop).unwrap();

    let instance_descriptor = InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..InstanceDescriptor::default()
    };
    let instance = wgpu::Instance::new(instance_descriptor);
    let surface = unsafe { instance.create_surface(&window).unwrap() };

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            features: wgpu::Features::default(),
            limits: wgpu::Limits::default(),
            label: None,
        },
        None,
    ))
    .unwrap();

    let capabilities = surface.get_capabilities(&adapter);
    let surface_format = *capabilities.formats.iter().find(|f| f.is_srgb()).unwrap();

    let size = window.inner_size();
    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &surface_config);

    let mut state = State::new(&window);
    state.set_pixels_per_point(window.scale_factor() as f32);

    // We use the egui_wgpu_backend crate as the render backend.
    let mut egui_rpass = Renderer::new(&device, surface_format, None, 1);

    // Display the demo application that ships with egui.
    #[cfg(feature = "demo")]
    let mut demo_app = egui_demo_lib::DemoWindows::default();

    let context = egui::Context::default();
    context.set_style(egui::Style::default());

    let _start_time = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        // Pass the winit events to the platform integration.
        if let WindowEvent { event, .. } = &event {
            let response = state.on_event(&context, event);
            if response.repaint {
                window.request_redraw();
            }
            if response.consumed {
                return;
            }
        }

        match event {
            RedrawRequested(..) => {
                let output_frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Outdated) => {
                        // This error occurs when the app is minimized on Windows.
                        // Silently return here to prevent spamming the console with:
                        // "The underlying surface has changed, and therefore the swap chain must be updated"
                        return;
                    }
                    Err(e) => {
                        eprintln!("Dropped frame with error: {}", e);
                        return;
                    }
                };
                let output_view = output_frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                // Begin to draw the UI frame.
                let input = state.take_egui_input(&window);
                context.begin_frame(input);

                // Draw the demo application.
                #[cfg(feature = "demo")]
                demo_app.ui(&context);

                // End the UI frame. We could now handle the output and draw the UI with the backend.
                let full_output = context.end_frame();
                let paint_jobs = context.tessellate(full_output.shapes);

                state.handle_platform_output(&window, &context, full_output.platform_output);

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

                // Upload all resources for the GPU.
                let screen_descriptor = ScreenDescriptor {
                    size_in_pixels: [surface_config.width, surface_config.height],
                    pixels_per_point: window.scale_factor() as f32,
                };
                let tdelta: egui::TexturesDelta = full_output.textures_delta;
                for (tid, deltas) in tdelta.set {
                    egui_rpass.update_texture(&device, &queue, tid, &deltas);
                }

                egui_rpass.update_buffers(
                    &device,
                    &queue,
                    &mut encoder,
                    &paint_jobs,
                    &screen_descriptor,
                );

                let color_attach = wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: Default::default(),
                };
                let renderpass_descriptor = wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(color_attach)],
                    ..Default::default()
                };
                let mut render_pass = encoder.begin_render_pass(&renderpass_descriptor);

                egui_rpass.render(&mut render_pass, &paint_jobs, &screen_descriptor);

                drop(render_pass);

                // Submit the commands.
                queue.submit(iter::once(encoder.finish()));

                // Redraw egui
                output_frame.present();

                for tid in tdelta.free {
                    egui_rpass.free_texture(&tid);
                }
            }
            MainEventsCleared => {
                window.request_redraw();
            }
            WindowEvent { event, .. } => match event {
                winit::event::WindowEvent::Resized(size) => {
                    // Resize with 0 width and height is used by winit to signal a minimize event on Windows.
                    // See: https://github.com/rust-windowing/winit/issues/208
                    // This solves an issue where the app would panic when minimizing on Windows.
                    if size.width > 0 && size.height > 0 {
                        surface_config.width = size.width;
                        surface_config.height = size.height;
                        surface.configure(&device, &surface_config);
                    }
                }
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            _ => (),
        }
    });
}
