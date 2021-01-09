extern crate nalgebra as na;
extern crate vk_mem as vma;

use std::{
        borrow::Cow,
        error::Error,
        ffi::{c_void, CStr, CString},
        io::Write,
        os::raw::c_char,
};

use ash::{
        extensions::{
                ext::DebugUtils,
                khr::{Surface, Swapchain},
        },
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
        vk::DebugUtilsMessengerCreateInfoEXT,
};
use chrono::Local;
use env_logger::Env;
use log::{info, trace, warn};
use memoffset::ptr::null;
use winit::{
        event::{Event, VirtualKeyCode, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::WindowBuilder,
};

macro_rules! cstring {
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

const ENABLE_VALIDATION_LAYERS: bool = true;

fn main() -> Result<(), Box<dyn Error>> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new().build(&event_loop)?;

        env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
                .format(|buf, record| {
                        writeln!(
                                buf,
                                "{} {}: {}",
                                Local::now().time().format("%H:%M:%S").to_string(),
                                record.level(),
                                record.args()
                        )
                })
                .init();

        let entry = ash::Entry::new()?;

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

        let surface_format = unsafe {
                let formats = surface_loader.get_physical_device_surface_formats(physical_device, surface)?;

                let find_format = |fmt: vk::Format, color_space: vk::ColorSpaceKHR| {
                        formats.iter().find(|f| f.format == fmt && f.color_space == color_space)
                };

                if let Some(fmt) = find_format(vk::Format::B8G8R8_SRGB, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        *fmt
                } else if let Some(fmt) = find_format(vk::Format::B8G8R8_UNORM, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        *fmt
                } else {
                        formats[0]
                }
        };

        let surface_capabilities =
                unsafe { surface_loader.get_physical_device_surface_capabilities(physical_device, surface)? };

        let max_img_count = match surface_capabilities.max_image_count {
                0 => u32::MAX,
                _ => surface_capabilities.max_image_count,
        };
        let desired_img_count = na::clamp(3, surface_capabilities.min_image_count, max_img_count);

        let surface_resolution = match surface_capabilities.current_extent.width {
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
                .image_color_space(surface_format.color_space)
                .image_format(surface_format.format)
                .image_extent(surface_resolution)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(pre_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present_mode)
                .clipped(true)
                .image_array_layers(1);

        let swapchain = unsafe { swch_loader.create_swapchain(&swch_cinfo, None)? };




















        let cmd_pool_cinfo = vk::CommandPoolCreateInfo::builder()
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                .queue_family_index(q_family_i);

        let cmd_pool = unsafe { device.create_command_pool(&cmd_pool_cinfo, None)? };


        let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                .command_pool(cmd_pool)
                .command_buffer_count(2)
                .level(vk::CommandBufferLevel::PRIMARY);

        let cmd_buffers = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

        let setup_cmd_buffer = cmd_buffers[0];
        let draw_cmd_buffer = cmd_buffers[1];

        let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

        let setup_commands_fence = unsafe { device.create_fence(&fence_cinfo, None)? };
        let draw_commands_fence = unsafe { device.create_fence(&fence_cinfo, None)? };

        let present_imgs = unsafe { swch_loader.get_swapchain_images(swapchain)? };
        let present_img_views: Vec<vk::ImageView> = present_imgs
                .iter()
                .map(|&img| {
                        let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                .image(img)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(surface_format.format)
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
                        width:  surface_resolution.width,
                        height: surface_resolution.height,
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
                        setup_commands_fence,
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

        let present_complete_semaphore = unsafe { device.create_semaphore(&semaphore_cinfo, None)? };
        let render_complete_semaphore = unsafe { device.create_semaphore(&semaphore_cinfo, None)? };




        event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::Wait;

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
                        Event::MainEventsCleared => {},
                        _ => (),
                }
        });

        return Ok(());
}

fn find_mem_type_index(
        device_mem_props: &vk::PhysicalDeviceMemoryProperties,
        mem_req: &vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
        device_mem_props.memory_types[..device_mem_props.memory_type_count as usize]
                .iter()
                .enumerate()
                .find(|(i, mem_type)| {
                        (((1 << i) & mem_req.memory_type_bits) != 0) && mem_type.property_flags.contains(flags)
                })
                .map(|(i, mem_type)| i as u32)
}

unsafe extern "system" fn vk_debug_callback(
        message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
        message_type: vk::DebugUtilsMessageTypeFlagsEXT,
        p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
        _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
        if message_type == vk::DebugUtilsMessageTypeFlagsEXT::GENERAL {
                return vk::FALSE;
        }

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

        device.wait_for_fences(&fences, true, u64::MAX)?;
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
