mod my_vec;
mod timer;
mod vertex;
mod vk_context;
mod vk_immutable_buffer;

extern crate nalgebra as na;
extern crate vk_mem as vma;

use std::{
        borrow::Cow,
        error::Error,
        ffi::{c_void, CStr, CString},
        io::Write,
        os::raw::c_char,
        process::Command,
        ptr,
        rc::Rc,
        time::{Duration, Instant},
};

use ash::{
        extensions::{
                ext::DebugUtils,
                khr::{Surface, Swapchain},
        },
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
        vk::{DebugUtilsMessengerCreateInfoEXT, Offset2D},
};
use chrono::Local;
use env_logger::Env;
use fps_counter::FPSCounter;
use log::{info, trace, warn};
use winit::{
        event::{Event, VirtualKeyCode, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, WindowBuilder},
};

use crate::{timer::Timer, vk_context::VkContext};

/*macro_rules! cstring {
        ($s:expr) => {
                CString::new($s).unwrap()
        };
}

macro_rules! cstring_arr {
        () => {
                []
        };
        ( $( $s:expr ),+ ) => {
                [ $( cstring!($s) )+ ]
        };
}

macro_rules! raw_cstring_arr {
        () => {
                ( [], Vec::<*const c_char>::new() )
        };
        ( $( $s:expr ),* ) => {{
                let arr = cstring_arr![ $( cstring!($s) )* ];
                let raw_arr: Vec<*const c_char> = arr.iter().map(|e| e.as_ptr()).collect();
                (arr, raw_arr)
        }};
}

const ENABLE_VALIDATION_LAYERS: bool = true;*/

fn main() -> Result<(), Box<dyn Error>> {
        env_logger::Builder::from_env(Env::default().default_filter_or("trace"))
                .format(|buf, record| {
                        writeln!(
                                buf,
                                "[{} {}] {}",
                                Local::now().time().format("%H:%M:%S").to_string(),
                                record.level(),
                                record.args()
                        )
                })
                .init();

        let event_loop = EventLoop::new();
        let window = Rc::new(WindowBuilder::new()
                .with_fullscreen(Some(Fullscreen::Borderless(None)))
                .with_fullscreen(Some(Fullscreen::Exclusive(
                        event_loop.primary_monitor().unwrap().video_modes().next().unwrap(),
                )))
                .with_fullscreen(None)
                .with_visible(false)
                .with_always_on_top(false)
                .with_min_inner_size(winit::dpi::PhysicalSize {
                        width:  240,
                        height: 240,
                })
                .build(&event_loop)?);
        trace!("Created window");

        let mut vk_context = VkContext::new(&window)?;




        let mut fps_ctr = FPSCounter::new();
        let mut last_print_fps = Instant::now();

        window.set_visible(true);
        event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::Poll;

                match event {
                        Event::WindowEvent {
                                window_id,
                                event,
                        } if window_id == window.id() => match event {
                                WindowEvent::Resized(size) => {
                                        vk_context.on_window_resize(size.width, size.height);
                                },
                                WindowEvent::CloseRequested => {
                                        *control_flow = ControlFlow::Exit;
                                },
                                WindowEvent::KeyboardInput {
                                        input, ..
                                } => {
                                        if let Some(virtual_keycode) = input.virtual_keycode {
                                                match virtual_keycode {
                                                        VirtualKeyCode::Escape => *control_flow = ControlFlow::Exit,
                                                        VirtualKeyCode::F => {},
                                                        _ => {},
                                                };
                                        }
                                },
                                _ => {},
                        },
                        Event::MainEventsCleared => unsafe {
                                let fps = fps_ctr.tick();
                                let now = Instant::now();
                                let time_since_last_print_fps = now - last_print_fps;

                                let print_interval = Duration::from_millis(250);
                                if time_since_last_print_fps > print_interval {
                                        last_print_fps += print_interval;
                                        info!("FPS: {}", fps);
                                }

                                vk_context.draw().expect("Error occurred while drawing");
                        },
                        _ => (),
                }
        });
















        trace!("Terminating program...");
        Ok(())
        /*let entry = ash::Entry::new()?;
        trace!("Created entry");

        let instance = unsafe {
                let (req_layers, req_layers_raw) = raw_cstring_arr!["VK_LAYER_KHRONOS_validation"];

                let mut req_extensions: Vec<CString> = ash_window::enumerate_required_extensions(&window)?
                        .iter()
                        .map(|ext| CString::from(*ext))
                        .collect();
                req_extensions.push(cstring!("VK_EXT_debug_utils"));
                let req_extensions_raw: Vec<*const c_char> = req_extensions.iter().map(|ext| ext.as_ptr()).collect();

                info!("Required layers: {:?}", req_layers);
                info!("Required extensions: {:?}", req_extensions);

                let app_name = cstring!("ash_sandbox");

                let app_info = vk::ApplicationInfo::builder()
                        .application_name(&app_name)
                        .application_version(vk::make_version(0, 1, 0))
                        .engine_name(&app_name)
                        .engine_version(vk::make_version(0, 1, 0))
                        .api_version(vk::make_version(1, 2, 0));

                let mut instance_cinfo = vk::InstanceCreateInfo::builder()
                        .application_info(&app_info)
                        .enabled_layer_names(&req_layers_raw)
                        .enabled_extension_names(&req_extensions_raw);

                let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                        .message_severity(
                                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                        )
                        .message_type(vk::DebugUtilsMessageTypeFlagsEXT::all())
                        .pfn_user_callback(Some(vk_debug_callback))
                        .build();

                if ENABLE_VALIDATION_LAYERS {
                        instance_cinfo.p_next = &debug_info as *const DebugUtilsMessengerCreateInfoEXT as *const c_void;
                }

                entry.create_instance(&instance_cinfo, None)?
        };
        trace!("Created VkInstance");



        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                                | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                )
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::all())
                .pfn_user_callback(Some(vk_debug_callback));

        let debug_utils_loader = DebugUtils::new(&entry, &instance);
        let debug_callback = unsafe { debug_utils_loader.create_debug_utils_messenger(&debug_info, None)? };

        let surface = unsafe { ash_window::create_surface(&entry, &instance, &window, None)? };
        let surface_loader = Surface::new(&entry, &instance);
        trace!("Created surface");


        let (physical_device, q_family_i) = unsafe {
                let ph_devices = instance.enumerate_physical_devices()?;

                ph_devices
                        .iter()
                        .map(|pd| {
                                instance.get_physical_device_queue_family_properties(*pd)
                                        .iter()
                                        .enumerate()
                                        .filter_map(|(i, ref info)| {
                                                if !info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                                                        return None;
                                                }

                                                if !surface_loader
                                                        .get_physical_device_surface_support(*pd, i as u32, surface)
                                                        .unwrap()
                                                {
                                                        return None;
                                                }

                                                Some((*pd, i as u32))
                                        })
                                        .next()
                        })
                        .filter_map(|v| v)
                        .find(|(pd, _i)| {
                                let name = CStr::from_ptr(
                                        instance.get_physical_device_properties(*pd).device_name.as_ptr(),
                                )
                                .to_str()
                                .unwrap();

                                name == "GeForce GTX 970"
                        })
                        //.next()
                        .expect("Couldn't find suitable device")
        };
        trace!("Chose VkPhysicalDevice");

        let req_device_extensions_raw = [Swapchain::name().as_ptr()];
        let req_device_features = vk::PhysicalDeviceFeatures::builder().shader_clip_distance(true);
        let q_families_priorities = [1.0];

        let q_cinfo = [vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(q_family_i)
                .queue_priorities(&q_families_priorities)
                .build()];

        let device_cinfo = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&q_cinfo)
                .enabled_extension_names(&req_device_extensions_raw)
                .enabled_features(&req_device_features);

        let device = unsafe { instance.create_device(physical_device, &device_cinfo, None)? };
        trace!("Created VkDevice");

        let present_queue = unsafe { device.get_device_queue(q_family_i, 0) };

        let allocator_cinfo = vma::AllocatorCreateInfo {
                physical_device: physical_device.clone(),
                device: device.clone(),
                instance: instance.clone(),
                flags: Default::default(),
                preferred_large_heap_block_size: 0,
                frame_in_use_count: 0,
                heap_size_limits: None,
        };

        let allocator = vma::Allocator::new(&allocator_cinfo)?;

        let swch_format = unsafe {
                let formats = surface_loader.get_physical_device_surface_formats(physical_device, surface)?;

                let find_format = |fmt: vk::Format, color_space: vk::ColorSpaceKHR| {
                        formats.iter().find(|f| f.format == fmt && f.color_space == color_space)
                };

                if let Some(fmt) = find_format(vk::Format::B8G8R8A8_UNORM, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        *fmt
                } else if let Some(fmt) = find_format(vk::Format::B8G8R8A8_SRGB, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        *fmt
                } else {
                        formats[0]
                }
        };

        info!("Chosen swapchain format: {:?}", swch_format);

        let surface_capabilities =
                unsafe { surface_loader.get_physical_device_surface_capabilities(physical_device, surface)? };

        let desired_img_count = na::clamp(
                3,
                surface_capabilities.min_image_count,
                match surface_capabilities.max_image_count {
                        0 => u32::MAX,
                        _ => surface_capabilities.max_image_count,
                },
        );

        info!("Final swapchain image count: {}", desired_img_count);

        let swch_extent = match surface_capabilities.current_extent.width {
                u32::MAX => vk::Extent2D {
                        width:  window.inner_size().width,
                        height: window.inner_size().height,
                },
                _ => surface_capabilities.current_extent,
        };

        let pre_transform = if surface_capabilities
                .supported_transforms
                .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
                vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
                surface_capabilities.current_transform
        };


        let present_mode = unsafe {
                let modes = surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?;

                let find_present_mode =
                        |mode: vk::PresentModeKHR| modes.iter().find(|&&m| m == vk::PresentModeKHR::MAILBOX).is_some();

                if find_present_mode(vk::PresentModeKHR::MAILBOX) {
                        vk::PresentModeKHR::MAILBOX
                } else if find_present_mode(vk::PresentModeKHR::IMMEDIATE) {
                        vk::PresentModeKHR::IMMEDIATE
                } else {
                        vk::PresentModeKHR::FIFO
                }
        };


        let swch_loader = Swapchain::new(&instance, &device);

        let swch_cinfo = vk::SwapchainCreateInfoKHR::builder()
                .surface(surface)
                .min_image_count(desired_img_count)
                .image_color_space(swch_format.color_space)
                .image_format(swch_format.format)
                .image_extent(swch_extent)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(pre_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present_mode)
                .clipped(true)
                .image_array_layers(1);

        let swapchain = unsafe { swch_loader.create_swapchain(&swch_cinfo, None)? };
        trace!("Created VkSwapchain");

        let swch_imgs = unsafe { swch_loader.get_swapchain_images(swapchain)? };
        let swch_img_count = swch_imgs.len();

        let swch_img_views: Vec<vk::ImageView> = swch_imgs
                .iter()
                .map(|&swch_img| {
                        let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                .image(swch_img)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(swch_format.format)
                                .components(vk::ComponentMapping::default())
                                .subresource_range(vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                });

                        unsafe {
                                device.create_image_view(&img_view_cinfo, None)
                                        .expect("Failed to create swapchain image view!")
                        }
                })
                .collect();





        let vert_shader = create_shader_module(&device, "res/shader/basic_shader.vert")?;
        let frag_shader = create_shader_module(&device, "res/shader/basic_shader.frag")?;

        let entry_point = CString::new("main")?;

        let shader_stages = [
                vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(vert_shader)
                        .name(&entry_point)
                        .build(),
                vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::FRAGMENT)
                        .module(frag_shader)
                        .name(&entry_point)
                        .build(),
        ];


        let vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder()
                .vertex_binding_descriptions(&[])
                .vertex_attribute_descriptions(&[]);


        let input_assembly_cinfo = vk::PipelineInputAssemblyStateCreateInfo::builder()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                .primitive_restart_enable(false);


        let viewports = [vk::Viewport {
                x:         0.0,
                y:         0.0,
                width:     swch_extent.width as f32,
                height:    swch_extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
        }];

        let scissors = [vk::Rect2D {
                offset: Offset2D {
                        x: 0, y: 0
                },
                extent: swch_extent,
        }];

        let viewport_state_cinfo = vk::PipelineViewportStateCreateInfo::builder()
                .viewports(&viewports)
                .scissors(&scissors);


        let rasterization_state_cinfo = vk::PipelineRasterizationStateCreateInfo::builder()
                .depth_clamp_enable(false)
                .rasterizer_discard_enable(false)
                .polygon_mode(vk::PolygonMode::FILL)
                .line_width(1.0)
                .cull_mode(vk::CullModeFlags::NONE)
                .front_face(vk::FrontFace::CLOCKWISE)
                .depth_bias_enable(false)
                .depth_bias_constant_factor(0.0)
                .depth_bias_clamp(0.0)
                .depth_bias_slope_factor(0.0);


        let multisample_state_cinfo = vk::PipelineMultisampleStateCreateInfo::builder()
                .rasterization_samples(vk::SampleCountFlags::TYPE_1)
                .sample_shading_enable(false);


        let depth_stencil_state_cinfo =
                vk::PipelineDepthStencilStateCreateInfo::builder().depth_compare_op(vk::CompareOp::LESS);


        let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::builder()
                .color_write_mask(vk::ColorComponentFlags::all())
                .blend_enable(false)
                .build()];


        let color_blend_state_cinfo = vk::PipelineColorBlendStateCreateInfo::builder()
                .attachments(&color_blend_attachments)
                .logic_op_enable(false);


        let dyn_states = [];

        let pipeline_dyn_state_cinfo = vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dyn_states);

        let pipeline_layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                .set_layouts(&[])
                .push_constant_ranges(&[]);

        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_cinfo, None)? };


        let color_attachment = vk::AttachmentDescription {
                flags:            Default::default(),
                format:           swch_format.format,
                samples:          vk::SampleCountFlags::TYPE_1,
                load_op:          vk::AttachmentLoadOp::CLEAR,
                store_op:         vk::AttachmentStoreOp::STORE,
                stencil_load_op:  vk::AttachmentLoadOp::DONT_CARE,
                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                initial_layout:   vk::ImageLayout::UNDEFINED,
                final_layout:     vk::ImageLayout::PRESENT_SRC_KHR,
        };

        let attachments = [color_attachment];


        let color_attachment_ref = vk::AttachmentReference {
                attachment: 0,
                layout:     vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        };

        let attachments_refs = [color_attachment_ref];

        let subpass_desc = vk::SubpassDescription::builder()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&attachments_refs);
        //.depth_stencil_attachment()
        //.input_attachments()
        //.resolve_attachments()
        //.preserve_attachments()

        let subpasses = [subpass_desc.build()];

        let subpass_dependencies = [vk::SubpassDependency {
                src_subpass:      vk::SUBPASS_EXTERNAL,
                dst_subpass:      0,
                src_stage_mask:   vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                dst_stage_mask:   vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                src_access_mask:  vk::AccessFlags::empty(),
                dst_access_mask:  vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                dependency_flags: vk::DependencyFlags::empty(),
        }];

        let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                .attachments(&attachments)
                .subpasses(&subpasses)
                .dependencies(&subpass_dependencies);

        let render_pass = unsafe { device.create_render_pass(&render_pass_cinfo, None)? };
        trace!("Created VkRenderPass");


        let graphics_pipeline_cinfo = [vk::GraphicsPipelineCreateInfo::builder()
                .stages(&shader_stages)
                .vertex_input_state(&vert_input_cinfo)
                .input_assembly_state(&input_assembly_cinfo)
                .viewport_state(&viewport_state_cinfo)
                .rasterization_state(&rasterization_state_cinfo)
                .multisample_state(&multisample_state_cinfo)
                //.depth_stencil_state(&)
                .color_blend_state(&color_blend_state_cinfo)
                .dynamic_state(&pipeline_dyn_state_cinfo)
                .layout(pipeline_layout)
                .render_pass(render_pass)
                .subpass(0)
                .base_pipeline_handle(vk::Pipeline::null())
                .base_pipeline_index(-1)
                .build()];


        let graphics_pipeline = unsafe {
                device.create_graphics_pipelines(vk::PipelineCache::null(), &graphics_pipeline_cinfo, None)
                        .expect("Failed to create graphics pipeline!")[0]
        };
        trace!("Created VkGraphicsPipeline");


        unsafe {
                device.destroy_shader_module(vert_shader, None);
                device.destroy_shader_module(frag_shader, None);
        }




        let framebuffers: Vec<vk::Framebuffer> = swch_img_views
                .iter()
                .map(|&swch_img_view| {
                        let swch_img_view = [swch_img_view];

                        let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                .render_pass(render_pass)
                                .attachments(&swch_img_view)
                                .width(swch_extent.width)
                                .height(swch_extent.height)
                                .layers(1);

                        unsafe {
                                device.create_framebuffer(&framebuffer_cinfo, None)
                                        .expect("Failed to create framebuffer!")
                        }
                })
                .collect();






        let cmd_pool_cinfo = vk::CommandPoolCreateInfo::builder()
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                .queue_family_index(q_family_i);

        let cmd_pool = unsafe { device.create_command_pool(&cmd_pool_cinfo, None)? };
        trace!("Created VkCommandPool");


        let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                .command_pool(cmd_pool)
                .command_buffer_count(1 + swch_imgs.len() as u32)
                .level(vk::CommandBufferLevel::PRIMARY);

        let mut cmd_buffers = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

        let setup_cmd_buffer = cmd_buffers[0];
        let draw_cmd_buffers: Vec<vk::CommandBuffer> = cmd_buffers[1..].iter().cloned().collect();

        let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

        let setup_cmd_buffer_reuse_fence = unsafe { device.create_fence(&fence_cinfo, None)? };
        let draw_cmd_buffer_reuse_fences: Vec<vk::Fence> = swch_imgs
                .iter()
                .map(|_| unsafe { device.create_fence(&fence_cinfo, None).unwrap() })
                .collect();

        let present_imgs = unsafe { swch_loader.get_swapchain_images(swapchain)? };
        let present_img_views: Vec<vk::ImageView> = present_imgs
                .iter()
                .map(|&img| {
                        let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                .image(img)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(swch_format.format)
                                .components(vk::ComponentMapping {
                                        r: vk::ComponentSwizzle::IDENTITY,
                                        g: vk::ComponentSwizzle::IDENTITY,
                                        b: vk::ComponentSwizzle::IDENTITY,
                                        a: vk::ComponentSwizzle::IDENTITY,
                                })
                                .subresource_range(vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                });

                        unsafe {
                                device.create_image_view(&img_view_cinfo, None)
                                        .expect("Failed to create swapchain image view!")
                        }
                })
                .collect();

        let depth_img_cinfo = vk::ImageCreateInfo::builder()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::D24_UNORM_S8_UINT)
                .extent(vk::Extent3D {
                        width:  swch_extent.width,
                        height: swch_extent.height,
                        depth:  1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let depth_img_ainfo = vma::AllocationCreateInfo {
                usage: vma::MemoryUsage::GpuOnly,
                ..Default::default()
        };

        let (depth_img, depth_img_alloc, alloc_info) = allocator.create_image(&depth_img_cinfo, &depth_img_ainfo)?;

        unsafe {
                record_and_submit_cmd_buffer(
                        &device,
                        setup_cmd_buffer,
                        setup_cmd_buffer_reuse_fence,
                        present_queue,
                        &[],
                        &[],
                        &[],
                        |device, setup_cmd_buffer| {
                                let dst_access_mask = vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;

                                let subresource_range = vk::ImageSubresourceRange::builder()
                                        .aspect_mask(vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL)
                                        .layer_count(1)
                                        .level_count(1)
                                        .build();

                                let layout_transition_barrier = vk::ImageMemoryBarrier::builder()
                                        .image(depth_img)
                                        .dst_access_mask(dst_access_mask)
                                        .old_layout(vk::ImageLayout::UNDEFINED)
                                        .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                                        .subresource_range(subresource_range);

                                device.cmd_pipeline_barrier(
                                        setup_cmd_buffer,
                                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                                        vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                        vk::DependencyFlags::empty(),
                                        &[],
                                        &[],
                                        &[layout_transition_barrier.build()],
                                )
                        },
                )?;
        }

        let depth_img_view_cinfo = vk::ImageViewCreateInfo::builder()
                .image(depth_img)
                .subresource_range(
                        vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL)
                                .layer_count(1)
                                .level_count(1)
                                .build(),
                )
                .format(depth_img_cinfo.format)
                .view_type(vk::ImageViewType::TYPE_2D);

        let depth_img_view = unsafe { device.create_image_view(&depth_img_view_cinfo, None)? };

        let semaphore_cinfo = vk::SemaphoreCreateInfo::default();

        let img_available_semaphores: Vec<vk::Semaphore> = swch_imgs
                .iter()
                .map(|_| unsafe {
                        device.create_semaphore(&semaphore_cinfo, None)
                                .expect("Failed to create image available semaphore")
                })
                .collect();

        let render_complete_semaphores: Vec<vk::Semaphore> = swch_imgs
                .iter()
                .map(|_| unsafe { device.create_semaphore(&semaphore_cinfo, None).unwrap() })
                .collect();

















        let mut frame_i: usize = 0;

        let mut fps_ctr = FPSCounter::new();
        let mut last_print_fps = Instant::now();

        event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::Poll;

                match event {
                        Event::WindowEvent {
                                event: WindowEvent::CloseRequested,
                                window_id,
                        } if window_id == window.id() => *control_flow = ControlFlow::Exit,
                        Event::WindowEvent {
                                event:
                                        WindowEvent::KeyboardInput {
                                                device_id: _,
                                                input,
                                                is_synthetic: _,
                                        },
                                ..
                        } => {
                                if let Some(virtual_keycode) = input.virtual_keycode {
                                        match virtual_keycode {
                                                VirtualKeyCode::Escape => *control_flow = ControlFlow::Exit,
                                                _ => {},
                                        };
                                };
                        },
                        Event::MainEventsCleared => unsafe {
                                let fps = fps_ctr.tick();
                                let now = Instant::now();
                                let time_since_last_print_fps = now - last_print_fps;

                                let print_interval = Duration::from_millis(100);
                                if time_since_last_print_fps > print_interval {
                                        last_print_fps += print_interval;
                                        info!("FPS: {}", fps);
                                }





                                let draw_cmd_buffer = draw_cmd_buffers[frame_i];
                                let draw_cmd_buffer_reuse_fence = draw_cmd_buffer_reuse_fences[frame_i];
                                let frame_img_avail_semaphore = img_available_semaphores[frame_i];
                                let frame_render_complete_semaphore = render_complete_semaphores[frame_i];

                                let (img_i, _suboptimal) = swch_loader
                                        .acquire_next_image(
                                                swapchain,
                                                u64::MAX,
                                                frame_img_avail_semaphore,
                                                vk::Fence::null(),
                                        )
                                        .expect("Failed to acquire image!");


                                let frame_framebuffer = framebuffers[img_i as usize];

                                let draw_binfo = vk::CommandBufferBeginInfo::builder()
                                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                                let clear_values = [vk::ClearValue {
                                        color: vk::ClearColorValue {
                                                float32: [0.1, 0.1, 0.1, 1.0],
                                        },
                                }];

                                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                        .render_pass(render_pass)
                                        .framebuffer(frame_framebuffer)
                                        .render_area(vk::Rect2D {
                                                offset: vk::Offset2D {
                                                        x: 0, y: 0
                                                },
                                                extent: swch_extent,
                                        })
                                        .clear_values(&clear_values);





                                record_and_submit_cmd_buffer(
                                        &device,
                                        draw_cmd_buffer,
                                        draw_cmd_buffer_reuse_fence,
                                        present_queue,
                                        &[frame_img_avail_semaphore],
                                        &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
                                        &[frame_render_complete_semaphore],
                                        |device, draw_cmd_buffer| {
                                                device.cmd_begin_render_pass(
                                                        draw_cmd_buffer,
                                                        &render_pass_binfo,
                                                        vk::SubpassContents::INLINE,
                                                );

                                                device.cmd_bind_pipeline(
                                                        draw_cmd_buffer,
                                                        vk::PipelineBindPoint::GRAPHICS,
                                                        graphics_pipeline,
                                                );

                                                for _ in 0..20000 {
                                                        device.cmd_draw(draw_cmd_buffer, 3, 1, 0, 0);
                                                }

                                                device.cmd_end_render_pass(draw_cmd_buffer);
                                        },
                                )
                                .unwrap();

                                swch_loader
                                        .queue_present(
                                                present_queue,
                                                &vk::PresentInfoKHR::builder()
                                                        .wait_semaphores(&[frame_render_complete_semaphore])
                                                        .swapchains(&[swapchain])
                                                        .image_indices(&[img_i]),
                                        )
                                        .expect("Error occurred while presenting image!");




                                frame_i = (frame_i + 1) % swch_img_count;
                        },
                        _ => (),
                }
        });*/
}

/*unsafe extern "system" fn vk_debug_callback(
        message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
        message_type: vk::DebugUtilsMessageTypeFlagsEXT,
        p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
        _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
        /*if message_type == vk::DebugUtilsMessageTypeFlagsEXT::GENERAL {
                return vk::FALSE;
        }*/

        let callback_data = *p_callback_data;
        let message_id_number: i32 = callback_data.message_id_number as i32;

        let message_id_name = if callback_data.p_message_id_name.is_null() {
                Cow::from("")
        } else {
                CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
        };

        let message = if callback_data.p_message.is_null() {
                Cow::from("")
        } else {
                CStr::from_ptr(callback_data.p_message).to_string_lossy()
        };

        println!(
                "{:?}:\n{:?} [{} ({})] : {}\n",
                message_severity,
                message_type,
                message_id_name,
                &message_id_number.to_string(),
                message,
        );

        vk::FALSE
}

unsafe fn record_and_submit_cmd_buffer<D, F>(
        device: &D,
        cmd_buffer: vk::CommandBuffer,
        cmd_buffer_fence: vk::Fence,
        submit_queue: vk::Queue,
        wait_semaphores: &[vk::Semaphore],
        wait_stages: &[vk::PipelineStageFlags],
        signal_semaphores: &[vk::Semaphore],
        f: F,
) -> VkResult<()>
where
        D: DeviceV1_0,
        F: FnOnce(&D, vk::CommandBuffer),
{
        let fences = [cmd_buffer_fence];

        {
                //let t = Timer::new("wait_for_fences took: ");

                device.wait_for_fences(&fences, true, u64::MAX)?;
        }
        device.reset_fences(&fences)?;
        device.reset_command_buffer(cmd_buffer, vk::CommandBufferResetFlags::RELEASE_RESOURCES)?;

        let cmd_buffer_binfo =
                vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        device.begin_command_buffer(cmd_buffer, &cmd_buffer_binfo)?;
        f(&device, cmd_buffer);
        device.end_command_buffer(cmd_buffer)?;

        let cmd_buffers = [cmd_buffer];

        let submit_info = vk::SubmitInfo::builder()
                .command_buffers(&cmd_buffers)
                .wait_semaphores(wait_semaphores)
                .wait_dst_stage_mask(wait_stages)
                .signal_semaphores(signal_semaphores);

        device.queue_submit(submit_queue, &[submit_info.build()], cmd_buffer_fence)?;

        Ok(())
}

fn create_shader_module<D: DeviceV1_0>(device: &D, path: &'static str) -> VkResult<vk::ShaderModule> {
        let compile_path = path.to_string() + ".spv";

        Command::new("res/misc/glslc.exe")
                .arg(path)
                .arg("-o")
                .arg(&compile_path)
                .spawn()
                .expect("Failed to compile shaders!");

        let shader_code = std::fs::read(&compile_path).expect("Failed to read shader binary file!");

        let mut shader_module_cinfo = vk::ShaderModuleCreateInfo::builder().build();
        shader_module_cinfo.code_size = shader_code.len();
        shader_module_cinfo.p_code = shader_code.as_ptr() as *const u32;

        assert_eq!(shader_code.len() % 4, 0, "Shader code is invalid!");

        unsafe { device.create_shader_module(&shader_module_cinfo, None) }
}
*/
