use std::{
        borrow::Cow,
        error::Error,
        ffi::{c_void, CStr, CString},
        os::raw::c_char,
        slice,
};

extern crate vk_mem as vma;

use std::{mem::size_of, ops::Deref, process::Command, rc::Rc};

use ash::{
        extensions::{
                ext::DebugUtils,
                khr::{Surface, Swapchain},
        },
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
        vk::Pipeline,
};
use log::{error, info, trace, warn};
use memoffset::mem::swap;
use winit::{dpi::PhysicalSize, window::Window};

use crate::{my_vec::*, timer::Timer, vertex::Vertex, vk_immutable_buffer::VkImmutableBuffer};

macro_rules! cstring {
        ($s:expr) => {
                CString::new($s).unwrap()
        };
}


pub struct VkContext {
        window: Rc<Window>,

        entry:    ash::Entry,
        instance: ash::Instance,

        debug_utils_loader:    Option<DebugUtils>,
        debug_utils_messenger: Option<vk::DebugUtilsMessengerEXT>,

        surface_loader: Surface,
        surface:        vk::SurfaceKHR,

        physical_device: vk::PhysicalDevice,
        device:          ash::Device,

        q_families_i: VkQueueFamilyIndices,
        queues:       VkQueues,

        allocator: vma::Allocator,

        swapchain: VkSwapchain,

        render_pass: vk::RenderPass,

        graphics_pipeline_layout: vk::PipelineLayout,
        graphics_pipeline:        vk::Pipeline,
        viewport:                 vk::Viewport,
        scissor:                  vk::Rect2D,

        cmd_pool:      vk::CommandPool,
        vertex_buffer: VkImmutableBuffer,
        index_buffer:  VkImmutableBuffer,

        draw_cmd_buffers:            Vec<VkReusableCommandBuffer>,
        img_avail_semaphores:        Vec<vk::Semaphore>,
        present_complete_semaphores: Vec<vk::Semaphore>,

        frame_i:            usize,
        recreate_swapchain: RecreateSwapchain,
}

#[cfg(all(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = true;
#[cfg(not(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = false;

impl VkContext {
        pub fn new(window: &Rc<Window>) -> Result<Self, Box<dyn Error>> {
                let entry = ash::Entry::new()?;

                let instance = Self::create_instance(window, &entry)?;
                trace!("Created VkInstance");

                let mut debug_utils_loader = None;
                let mut debug_utils_messenger = None;
                if ENABLE_VALIDATION_LAYERS {
                        debug_utils_loader = Some(DebugUtils::new(&entry, &instance));

                        debug_utils_messenger = Some(Self::create_debug_utils_messenger(
                                debug_utils_loader.as_ref().unwrap(),
                        )?);

                        trace!("Created VkDebugUtilsMessenger");
                };

                let surface_loader = Surface::new(&entry, &instance);
                let surface = unsafe { ash_window::create_surface(&entry, &instance, window.deref().deref(), None)? };
                trace!("Created VkSurface");

                let (physical_device, q_families_i) =
                        Self::choose_physical_device(&instance, &surface_loader, surface)?;

                trace!("Chose VkPhysicalDevice");
                info!("Chosen physical device: {:?}", unsafe {
                        CStr::from_ptr(
                                instance.get_physical_device_properties(physical_device)
                                        .device_name
                                        .as_ptr(),
                        )
                });

                let (device, queues) = Self::create_device(&instance, physical_device, &q_families_i)?;
                trace!("Created VkDevice");

                let allocator = Self::create_allocator(&instance, physical_device, &device)?;
                trace!("Created VmaAllocator");

                let mut swapchain = VkSwapchain::new(
                        window,
                        &instance,
                        &surface_loader,
                        surface,
                        physical_device,
                        &device,
                        vk::SwapchainKHR::null(),
                )?;
                trace!("Created VkSwapchain");

                let (viewport, scissor) = Self::create_viewport_and_scissor(swapchain.extent);

                let render_pass = Self::create_render_pass(&device, swapchain.format.format)?;
                trace!("Created VkRenderPass");

                swapchain.framebuffers = Self::create_framebuffers(&device, &swapchain, render_pass)?;
                trace!("Created VkFramebuffers");

                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(&device)?;
                trace!("Created VkGraphicsPipelineLayout");

                let graphics_pipeline = Self::create_graphics_pipeline(
                        &device,
                        &viewport,
                        &scissor,
                        render_pass,
                        graphics_pipeline_layout,
                )?;
                trace!("Created VkGraphicsPipeline");

                let cmd_pool = Self::create_command_pool(&device, &q_families_i)?;
                trace!("Created VkCommandPool");

                let setup_cmd_buffer = VkReusableCommandBuffer::new(&device, cmd_pool)?;
                let draw_cmd_buffers = VkReusableCommandBuffer::new_array(&device, cmd_pool, swapchain.img_count)?;
                trace!("Allocated VkCommandBuffers");

                let vertex_buffer =
                        Self::create_vertex_buffer(&device, &allocator, &q_families_i, &queues, &setup_cmd_buffer)?;
                trace!("Created vertex buffer");
                let index_buffer =
                        Self::create_index_buffer(&device, &allocator, &q_families_i, &queues, &setup_cmd_buffer)?;
                trace!("Created vertex buffer");

                let (img_avail_semaphores, present_complete_semaphores) =
                        Self::create_sync_objects(&device, &swapchain)?;
                trace!("Created VkSemaphores");

                Ok(Self {
                        window: Rc::clone(window),

                        entry,
                        instance,

                        debug_utils_loader,
                        debug_utils_messenger,

                        surface_loader,
                        surface,

                        physical_device,
                        device,

                        q_families_i,
                        queues,

                        allocator,

                        swapchain,

                        render_pass,

                        graphics_pipeline_layout,
                        graphics_pipeline,
                        viewport,
                        scissor,

                        cmd_pool,
                        vertex_buffer,
                        index_buffer,

                        draw_cmd_buffers,
                        img_avail_semaphores,
                        present_complete_semaphores,

                        frame_i: 0,
                        recreate_swapchain: RecreateSwapchain::No,
                })
        }

        pub fn draw(&mut self) -> VkResult<()> {
                let window_size = self.window.inner_size();
                if window_size.width == 0 || window_size.height == 0 {
                        return Ok(());
                }

                match self.recreate_swapchain {
                        RecreateSwapchain::No => (),
                        RecreateSwapchain::Swapchain => unsafe {
                                self.recreate_swapchain()?;
                                self.recreate_swapchain = RecreateSwapchain::No;
                        },
                        RecreateSwapchain::SwapchainAndPipeline => unsafe {
                                self.recreate_swapchain_and_pipeline()?;
                                self.recreate_swapchain = RecreateSwapchain::No;
                        },
                };

                let frame_img_avail_semaphore = self.img_avail_semaphores[self.frame_i];

                let (img_i, suboptimal) = unsafe {
                        self.swapchain
                                .acquire_next_image(u64::MAX, frame_img_avail_semaphore, vk::Fence::null())?
                };

                if suboptimal {
                        unsafe {
                                self.recreate_swapchain()?;
                        }
                }

                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.frame_i];
                let frame_present_complete_semaphore = self.present_complete_semaphores[self.frame_i];

                let frame_framebuffer = self.swapchain.framebuffers[img_i as usize];

                let clear_values = [vk::ClearValue {
                        color: vk::ClearColorValue {
                                float32: [0.1, 0.1, 0.1, 1.0],
                        },
                }];

                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                        .render_pass(self.render_pass)
                        .framebuffer(frame_framebuffer)
                        .render_area(self.scissor)
                        .clear_values(&clear_values);




                frame_draw_cmd_buffer.record_and_submit(
                        &self.device,
                        self.queues.graphics,
                        &[frame_img_avail_semaphore],
                        &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
                        &[frame_present_complete_semaphore],
                        |device, draw_cmd_buffer| unsafe {
                                device.cmd_begin_render_pass(
                                        draw_cmd_buffer,
                                        &render_pass_binfo,
                                        vk::SubpassContents::INLINE,
                                );

                                device.cmd_bind_pipeline(
                                        draw_cmd_buffer,
                                        vk::PipelineBindPoint::GRAPHICS,
                                        self.graphics_pipeline,
                                );

                                device.cmd_set_viewport(
                                        draw_cmd_buffer,
                                        0,
                                        slice::from_raw_parts(&self.viewport as *const _, 1),
                                );
                                device.cmd_set_scissor(
                                        draw_cmd_buffer,
                                        0,
                                        slice::from_raw_parts(&self.scissor as *const _, 1),
                                );

                                device.cmd_bind_vertex_buffers(draw_cmd_buffer, 0, &[self.vertex_buffer.handle], &[0]);
                                device.cmd_bind_index_buffer(
                                        draw_cmd_buffer,
                                        self.index_buffer.handle,
                                        0,
                                        vk::IndexType::UINT32,
                                );

                                for _ in 0..1 {
                                        device.cmd_draw_indexed(draw_cmd_buffer, 6, 1, 0, 0, 0);
                                }

                                device.cmd_end_render_pass(draw_cmd_buffer);
                        },
                )?;

                unsafe {
                        match self.swapchain.queue_present(
                                self.queues.graphics,
                                &vk::PresentInfoKHR::builder()
                                        .wait_semaphores(&[frame_present_complete_semaphore])
                                        .swapchains(&[self.swapchain.handle])
                                        .image_indices(&[img_i]),
                        ) {
                                Ok(true) => {
                                        // Suboptimal
                                        if let RecreateSwapchain::No = self.recreate_swapchain {
                                                self.recreate_swapchain = RecreateSwapchain::Swapchain
                                        }
                                },
                                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                        self.recreate_swapchain = RecreateSwapchain::SwapchainAndPipeline;
                                },
                                Err(err) => return Err(err),
                                _ => {},
                        };
                }




                self.frame_i = (self.frame_i + 1) % (self.swapchain.img_count as usize);

                Ok(())
        }

        pub fn on_window_resize(&mut self, width: u32, height: u32) {
                if width == 0 || height == 0 {
                        return;
                }

                if width != self.swapchain.extent.width || height != self.swapchain.extent.height {
                        self.recreate_swapchain = RecreateSwapchain::Swapchain;
                        self.viewport.x = 0.0;
                        self.viewport.y = height as f32;
                        self.viewport.width = width as f32;
                        self.viewport.height = -(height as f32);
                        self.scissor.extent.width = width;
                        self.scissor.extent.height = height;
                }
        }

        unsafe fn recreate_swapchain(&mut self) -> VkResult<()> {
                trace!("Recreating VkSwapchain");

                let t = Timer::new("Recreated VkSwapchain in: ");

                {
                        let t = Timer::new("Recreate swapchain wait idle took: ");
                        self.device.device_wait_idle()?;
                }

                let old_swch_format = self.swapchain.format.format;

                self.swapchain = VkSwapchain::new(
                        &self.window,
                        &self.instance,
                        &self.surface_loader,
                        self.surface,
                        self.physical_device,
                        &self.device,
                        self.swapchain.handle,
                )?;
                if old_swch_format != self.swapchain.format.format {
                        self.render_pass = Self::create_render_pass(&self.device, self.swapchain.format.format)?;
                }
                self.swapchain.framebuffers =
                        Self::create_framebuffers(&self.device, &self.swapchain, self.render_pass)?;

                Ok(())
        }

        unsafe fn recreate_pipeline(&mut self) -> VkResult<()> {
                trace!("Recreating VkPipeline");

                let t = Timer::new("Recreated VkPipeline in: ");

                {
                        let t = Timer::new("Recreate pipeline wait idle took: ");
                        self.device.device_wait_idle()?;
                }

                self.graphics_pipeline_layout = Self::create_graphics_pipeline_layout(&self.device)?;

                self.graphics_pipeline = Self::create_graphics_pipeline(
                        &self.device,
                        &self.viewport,
                        &self.scissor,
                        self.render_pass,
                        self.graphics_pipeline_layout,
                )?;

                Ok(())
        }

        unsafe fn recreate_swapchain_and_pipeline(&mut self) -> VkResult<()> {
                self.recreate_swapchain()?;
                self.recreate_pipeline()?;

                Ok(())
        }

        fn create_instance(window: &Window, entry: &ash::Entry) -> Result<ash::Instance, Box<dyn Error>> {
                unsafe {
                        let mut req_layers = Vec::new();
                        if ENABLE_VALIDATION_LAYERS {
                                req_layers.push(cstring!("VK_LAYER_KHRONOS_validation"));
                        }

                        let mut req_extensions: Vec<CString> = ash_window::enumerate_required_extensions(window)?
                                .iter()
                                .map(|ext| CString::from(*ext))
                                .collect();
                        req_extensions.push(cstring!("VK_EXT_debug_utils"));


                        let req_layers_raw: Vec<*const c_char> =
                                req_layers.iter().map(|layer| layer.as_ptr()).collect();

                        let req_extensions_raw: Vec<*const c_char> =
                                req_extensions.iter().map(|ext| ext.as_ptr()).collect();

                        info!("Required layers: {:?}", req_layers);
                        info!("Required extensions: {:?}", req_extensions);

                        let app_name = cstring!("ash_sandbox");

                        let app_info = vk::ApplicationInfo::builder()
                                .application_name(&app_name)
                                .application_version(vk::make_version(1, 0, 0))
                                .engine_name(&app_name)
                                .engine_version(vk::make_version(1, 0, 0))
                                .api_version(vk::make_version(1, 1, 0));

                        let mut instance_cinfo = vk::InstanceCreateInfo::builder()
                                .application_info(&app_info)
                                .enabled_layer_names(&req_layers_raw)
                                .enabled_extension_names(&req_extensions_raw);

                        let debug_info = Self::create_debug_utils_messenger_cinfo();

                        if ENABLE_VALIDATION_LAYERS {
                                instance_cinfo.p_next =
                                        &debug_info as *const vk::DebugUtilsMessengerCreateInfoEXT as *const c_void;
                        }

                        entry.create_instance(&instance_cinfo, None).map_err(|e| e.into())
                }
        }

        fn create_debug_utils_messenger_cinfo() -> vk::DebugUtilsMessengerCreateInfoEXT {
                vk::DebugUtilsMessengerCreateInfoEXT::builder()
                        .message_severity(
                                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                        )
                        .message_type(vk::DebugUtilsMessageTypeFlagsEXT::all())
                        .pfn_user_callback(Some(vk_debug_callback))
                        .build()
        }

        fn create_debug_utils_messenger(
                debug_utils_loader: &DebugUtils,
        ) -> Result<vk::DebugUtilsMessengerEXT, Box<dyn Error>> {
                let debug_cinfo = Self::create_debug_utils_messenger_cinfo();

                unsafe { debug_utils_loader.create_debug_utils_messenger(&debug_cinfo, None) }.map_err(|e| e.into())
        }

        fn choose_physical_device(
                instance: &ash::Instance,
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
        ) -> Result<(vk::PhysicalDevice, VkQueueFamilyIndices), Box<dyn Error>> {
                Ok(unsafe {
                        let ph_devices = instance.enumerate_physical_devices()?;

                        ph_devices
                                .iter()
                                .filter_map(|&pd| Self::is_device_suitable(instance, surface_loader, surface, pd))
                                .find(|&(pd, _)| {
                                        let name = CStr::from_ptr(
                                                instance.get_physical_device_properties(pd).device_name.as_ptr(),
                                        )
                                        .to_str()
                                        .unwrap();

                                        name == "GeForce GTX 970"
                                })
                                .expect("Couldn't find suitable device")
                })
        }

        fn is_device_suitable(
                instance: &ash::Instance,
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
                pd: vk::PhysicalDevice,
        ) -> Option<(vk::PhysicalDevice, VkQueueFamilyIndices)> {
                let q_families_i = match VkQueueFamilyIndices::new(instance, surface_loader, surface, pd) {
                        Some(q_families_i) => q_families_i,
                        None => return None,
                };

                Some((pd, q_families_i))
        }

        fn create_device(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                q_families_i: &VkQueueFamilyIndices,
        ) -> Result<(ash::Device, VkQueues), Box<dyn Error>> {
                let req_device_extensions_raw = vec![Swapchain::name().as_ptr()];
                let req_device_features = vk::PhysicalDeviceFeatures::builder().shader_clip_distance(true);

                let queue_priorities;

                let device_q_cinfos = if q_families_i.graphics == q_families_i.present {
                        queue_priorities = vec![1.0];

                        vec![vk::DeviceQueueCreateInfo::builder()
                                .queue_family_index(q_families_i.graphics)
                                .queue_priorities(&queue_priorities)
                                .build()]
                } else {
                        queue_priorities = vec![0.75, 0.25];

                        vec![
                                vk::DeviceQueueCreateInfo::builder()
                                        .queue_family_index(q_families_i.graphics)
                                        .queue_priorities(&queue_priorities[0..1])
                                        .build(),
                                vk::DeviceQueueCreateInfo::builder()
                                        .queue_family_index(q_families_i.present)
                                        .queue_priorities(&queue_priorities[1..2])
                                        .build(),
                        ]
                };

                let device_cinfo = vk::DeviceCreateInfo::builder()
                        .queue_create_infos(&device_q_cinfos)
                        .enabled_extension_names(&req_device_extensions_raw)
                        .enabled_features(&req_device_features);

                let device = unsafe { instance.create_device(physical_device, &device_cinfo, None)? };

                let queues = VkQueues {
                        graphics: unsafe { device.get_device_queue(q_families_i.graphics, 0) },
                        present:  unsafe { device.get_device_queue(q_families_i.present, 0) },
                };

                Ok((device, queues))
        }

        fn create_allocator(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                device: &ash::Device,
        ) -> Result<vma::Allocator, Box<dyn Error>> {
                let allocator_cinfo = vma::AllocatorCreateInfo {
                        physical_device,
                        device: device.clone(),
                        instance: instance.clone(),
                        flags: vma::AllocatorCreateFlags::default(),
                        preferred_large_heap_block_size: 0,
                        frame_in_use_count: 0,
                        heap_size_limits: None,
                };

                vma::Allocator::new(&allocator_cinfo).map_err(|e| e.into())
        }

        fn create_vertex_buffer(
                device: &ash::Device,
                allocator: &vma::Allocator,
                q_families_i: &VkQueueFamilyIndices,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkImmutableBuffer, Box<dyn Error>> {
                let data = [
                        Vertex {
                                pos:       Vec3::new(-0.5, -0.5, 0.0),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos:       Vec3::new(-0.5, 0.5, 0.0),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, 0.5, 0.0),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos:       Vec3::new(0.5, -0.5, 0.0),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                ];

                VkImmutableBuffer::from_slice(
                        device,
                        allocator,
                        setup_cmd_buffer,
                        q_families_i.graphics,
                        queues.graphics,
                        vk::BufferUsageFlags::VERTEX_BUFFER,
                        &data,
                )
        }

        fn create_index_buffer(
                device: &ash::Device,
                allocator: &vma::Allocator,
                q_families_i: &VkQueueFamilyIndices,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkImmutableBuffer, Box<dyn Error>> {
                let data: [u32; 6] = [3, 0, 1, 1, 2, 3];

                VkImmutableBuffer::from_slice(
                        device,
                        allocator,
                        setup_cmd_buffer,
                        q_families_i.graphics,
                        queues.graphics,
                        vk::BufferUsageFlags::INDEX_BUFFER,
                        &data,
                )
        }

        fn create_render_pass(device: &ash::Device, swapchain_format: vk::Format) -> VkResult<vk::RenderPass> {
                let attachments = [vk::AttachmentDescription {
                        flags:            vk::AttachmentDescriptionFlags::empty(),
                        format:           swapchain_format,
                        samples:          vk::SampleCountFlags::TYPE_1,
                        load_op:          vk::AttachmentLoadOp::CLEAR,
                        store_op:         vk::AttachmentStoreOp::STORE,
                        stencil_load_op:  vk::AttachmentLoadOp::DONT_CARE,
                        stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                        initial_layout:   vk::ImageLayout::UNDEFINED,
                        final_layout:     vk::ImageLayout::PRESENT_SRC_KHR,
                }];

                let attachments_refs = [vk::AttachmentReference {
                        attachment: 0,
                        layout:     vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                }];

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        .color_attachments(&attachments_refs)
                        //.depth_stencil_attachment()
                        //.input_attachments(&[])
                        //.resolve_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];


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
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { device.create_render_pass(&render_pass_cinfo, None) }
        }

        fn create_viewport_and_scissor(swapchain_extent: vk::Extent2D) -> (vk::Viewport, vk::Rect2D) {
                (
                        vk::Viewport {
                                x:         0.0,
                                y:         swapchain_extent.height as f32,
                                width:     swapchain_extent.width as f32,
                                height:    -(swapchain_extent.height as f32),
                                min_depth: 0.0,
                                max_depth: 1.0,
                        },
                        vk::Rect2D {
                                offset: vk::Offset2D {
                                        x: 0, y: 0
                                },
                                extent: swapchain_extent,
                        },
                )
        }

        fn create_graphics_pipeline_layout(device: &ash::Device) -> VkResult<vk::PipelineLayout> {
                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        /*.push_constant_ranges(&[])
                        .set_layouts(&[])*/;

                unsafe { device.create_pipeline_layout(&layout_cinfo, None) }
        }

        fn create_graphics_pipeline(
                device: &ash::Device,
                viewport: &vk::Viewport,
                scissor: &vk::Rect2D,
                render_pass: vk::RenderPass,
                pipeline_layout: vk::PipelineLayout,
        ) -> VkResult<vk::Pipeline> {
                let vert_shader = create_shader_module(device, "res/shader/basic_shader.vert")?;
                let frag_shader = create_shader_module(device, "res/shader/basic_shader.frag")?;

                let entry_point = CString::new("main").unwrap();

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


                let vert_binding_desc = [Vertex::vk_binding_description()];
                let vert_attrib_descs = Vertex::vk_attribute_descriptions();
                let vert_input_cinfo = vk::PipelineVertexInputStateCreateInfo::builder()
                        .vertex_binding_descriptions(&vert_binding_desc)
                        .vertex_attribute_descriptions(&vert_attrib_descs);


                let input_assembly_cinfo = vk::PipelineInputAssemblyStateCreateInfo::builder()
                        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                        .primitive_restart_enable(false);


                let viewport_state_cinfo = unsafe {
                        vk::PipelineViewportStateCreateInfo::builder()
                                .viewports(slice::from_raw_parts(viewport as *const _, 1))
                                .scissors(slice::from_raw_parts(scissor as *const _, 1))
                };


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


                let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
                let pipeline_dyn_state_cinfo =
                        vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dyn_states);



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
                        .build()];

                let graphics_pipelines = unsafe {
                        device.create_graphics_pipelines(vk::PipelineCache::null(), &graphics_pipeline_cinfo, None)
                };

                unsafe {
                        device.destroy_shader_module(vert_shader, None);
                        device.destroy_shader_module(frag_shader, None);
                }

                match graphics_pipelines {
                        Ok(graphics_pipelines) => {
                                assert_eq!(graphics_pipelines.len(), 1);

                                Ok(graphics_pipelines[0])
                        },
                        Err((graphics_pipelines, err)) => {
                                assert_eq!(graphics_pipelines.len(), 1);

                                Err(err)
                        },
                }
        }

        fn create_framebuffers(
                device: &ash::Device,
                swapchain: &VkSwapchain,
                render_pass: vk::RenderPass,
        ) -> VkResult<Vec<vk::Framebuffer>> {
                swapchain
                        .img_views
                        .iter()
                        .map(|&swch_img_view| {
                                let swch_img_view = [swch_img_view];

                                let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                        .render_pass(render_pass)
                                        .attachments(&swch_img_view)
                                        .width(swapchain.extent.width)
                                        .height(swapchain.extent.height)
                                        .layers(1);

                                unsafe { device.create_framebuffer(&framebuffer_cinfo, None) }
                        })
                        .collect()
        }

        fn create_command_pool(device: &ash::Device, q_families_i: &VkQueueFamilyIndices) -> VkResult<vk::CommandPool> {
                let cmd_pool_cinfo = vk::CommandPoolCreateInfo::builder()
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                        .queue_family_index(q_families_i.graphics);

                unsafe { device.create_command_pool(&cmd_pool_cinfo, None) }
        }

        fn create_sync_objects(
                device: &ash::Device,
                swapchain: &VkSwapchain,
        ) -> VkResult<(Vec<vk::Semaphore>, Vec<vk::Semaphore>)> {
                fn create_n_semaphores(
                        device: &ash::Device,
                        semaphore_cinfo: &vk::SemaphoreCreateInfo,
                        n: usize,
                ) -> VkResult<Vec<vk::Semaphore>> {
                        let mut semaphores = Vec::with_capacity(n);

                        for _ in 0..n {
                                semaphores.push(unsafe { device.create_semaphore(semaphore_cinfo, None)? });
                        }

                        Ok(semaphores)
                }

                let semaphore_cinfo = vk::SemaphoreCreateInfo::builder().build();

                Ok((
                        create_n_semaphores(device, &semaphore_cinfo, swapchain.img_count as usize)?,
                        create_n_semaphores(device, &semaphore_cinfo, swapchain.img_count as usize)?,
                ))
        }
}



impl Drop for VkContext {
        fn drop(&mut self) {
                unsafe {
                        let _ = self.device.device_wait_idle();



                        for &s in &self.img_avail_semaphores {
                                self.device.destroy_semaphore(s, None);
                        }
                        for &s in &self.present_complete_semaphores {
                                self.device.destroy_semaphore(s, None);
                        }

                        self.device.destroy_command_pool(self.cmd_pool, None);

                        self.device.destroy_pipeline(self.graphics_pipeline, None);
                        self.device.destroy_pipeline_layout(self.graphics_pipeline_layout, None);

                        self.device.destroy_render_pass(self.render_pass, None);

                        self.swapchain.destroy(&self.device);

                        let _ = self.vertex_buffer.destroy(&self.allocator);
                        let _ = self.index_buffer.destroy(&self.allocator);
                        self.allocator.destroy();

                        self.device.destroy_device(None);

                        self.surface_loader.destroy_surface(self.surface, None);

                        if let Some(debug_utils_messenger) = self.debug_utils_messenger.take() {
                                self.debug_utils_loader
                                        .as_ref()
                                        .unwrap()
                                        .destroy_debug_utils_messenger(debug_utils_messenger, None);
                        }

                        self.instance.destroy_instance(None);
                }
        }
}





enum RecreateSwapchain {
        No,
        Swapchain,
        SwapchainAndPipeline,
}



struct VkQueueFamilyIndices {
        graphics: u32,
        present:  u32,
}

impl VkQueueFamilyIndices {
        fn new(
                instance: &ash::Instance,
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
                pd: vk::PhysicalDevice,
        ) -> Option<Self> {
                let q_families_props = unsafe { instance.get_physical_device_queue_family_properties(pd) };

                fn find_queue_family<F>(q_families_props: &[vk::QueueFamilyProperties], cond: F) -> Option<u32>
                where F: Fn(usize, &vk::QueueFamilyProperties) -> bool {
                        q_families_props
                                .iter()
                                .enumerate()
                                .filter_map(
                                        |(i, q_fam_props)| {
                                                if cond(i, q_fam_props) {
                                                        Some(i as u32)
                                                } else {
                                                        None
                                                }
                                        },
                                )
                                .next()
                }

                let supports_graphics = |_i: usize, q_fam_props: &vk::QueueFamilyProperties| {
                        q_fam_props.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                };

                let supports_present = |i: usize, _q_fam_props: &vk::QueueFamilyProperties| unsafe {
                        !surface_loader
                                .get_physical_device_surface_support(pd, i as u32, surface)
                                .unwrap()
                };

                let supports_both = |i: usize, q_fam_props: &vk::QueueFamilyProperties| {
                        supports_graphics(i, q_fam_props) && supports_present(i, q_fam_props)
                };


                let graphics_and_present = find_queue_family(&q_families_props, supports_both);

                if let Some(graphics_and_present) = graphics_and_present {
                        return Some(Self {
                                graphics: graphics_and_present,
                                present:  graphics_and_present,
                        });
                }


                let graphics = find_queue_family(&q_families_props, supports_graphics);
                let present = find_queue_family(&q_families_props, supports_present);

                if let (Some(graphics), Some(present)) = (graphics, present) {
                        return Some(Self {
                                graphics,
                                present,
                        });
                }

                None
        }
}





struct VkQueues {
        graphics: vk::Queue,
        present:  vk::Queue,
}





struct VkSwapchain {
        loader:       Swapchain,
        handle:       vk::SwapchainKHR,
        format:       vk::SurfaceFormatKHR,
        extent:       vk::Extent2D,
        present_mode: vk::PresentModeKHR,
        imgs:         Vec<vk::Image>,
        img_count:    u32,
        img_views:    Vec<vk::ImageView>,
        framebuffers: Vec<vk::Framebuffer>,
}



impl VkSwapchain {
        fn new(
                window: &Window,
                instance: &ash::Instance,
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
                physical_device: vk::PhysicalDevice,
                device: &ash::Device,
                old_swapchain: vk::SwapchainKHR,
        ) -> VkResult<Self> {
                let format = Self::choose_format(surface_loader, surface, physical_device)?;
                info!("Swapchain format ({:?})", format);

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

                info!("Swapchain image count: {}", desired_img_count);

                let extent = match surface_capabilities.current_extent.width {
                        u32::MAX => vk::Extent2D {
                                width:  window.inner_size().width,
                                height: window.inner_size().height,
                        },
                        _ => surface_capabilities.current_extent,
                };
                info!("Swapchain extent: {:?}", extent);

                let pre_transform = surface_capabilities.current_transform;

                let present_mode = Self::choose_present_mode(&surface_loader, surface, physical_device)?;
                info!("VkSwapchain present mode: {:?}", present_mode);

                let loader = Swapchain::new(instance, device);

                let swch_cinfo = vk::SwapchainCreateInfoKHR::builder()
                        .surface(surface)
                        .min_image_count(desired_img_count)
                        .image_color_space(format.color_space)
                        .image_format(format.format)
                        .image_extent(extent)
                        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .pre_transform(pre_transform)
                        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                        .present_mode(present_mode)
                        .clipped(true)
                        .image_array_layers(1)
                        .old_swapchain(old_swapchain);

                let handle = unsafe { loader.create_swapchain(&swch_cinfo, None)? };

                let imgs = unsafe { loader.get_swapchain_images(handle)? };

                let img_count = imgs.len() as u32;

                let img_views = imgs
                        .iter()
                        .map(|&img| {
                                let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                        .image(img)
                                        .view_type(vk::ImageViewType::TYPE_2D)
                                        .format(format.format)
                                        .components(vk::ComponentMapping::default())
                                        .subresource_range(vk::ImageSubresourceRange {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                base_mip_level:   0,
                                                level_count:      1,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        });

                                unsafe { device.create_image_view(&img_view_cinfo, None) }
                        })
                        .collect::<VkResult<Vec<vk::ImageView>>>()?;

                Ok(Self {
                        loader,
                        handle,
                        format,
                        extent,
                        present_mode,
                        imgs,
                        img_count,
                        img_views,
                        framebuffers: vec![],
                })
        }

        unsafe fn acquire_next_image(
                &self,
                timeout: u64,
                semaphore: vk::Semaphore,
                fence: vk::Fence,
        ) -> VkResult<(u32, bool)> {
                self.loader.acquire_next_image(self.handle, timeout, semaphore, fence)
        }

        unsafe fn queue_present(&self, queue: vk::Queue, present_info: &vk::PresentInfoKHR) -> VkResult<bool> {
                self.loader.queue_present(queue, present_info)
        }

        fn destroy(&mut self, device: &ash::Device) {
                unsafe {
                        assert_eq!(self.imgs.len(), self.img_views.len());
                        assert_eq!(self.imgs.len(), self.framebuffers.len());

                        for i in 0..self.imgs.len() {
                                device.destroy_image_view(self.img_views[i], None);
                                device.destroy_framebuffer(self.framebuffers[i], None);
                        }

                        self.loader.destroy_swapchain(self.handle, None)
                };
        }

        fn choose_format(
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::SurfaceFormatKHR> {
                let formats = unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface)? };

                let find_format = |fmt: vk::Format, color_space: vk::ColorSpaceKHR| {
                        formats.iter().find(|f| f.format == fmt && f.color_space == color_space)
                };

                if let Some(&fmt) = find_format(vk::Format::B8G8R8A8_UNORM, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        Ok(fmt)
                } else if let Some(&fmt) = find_format(vk::Format::B8G8R8A8_SRGB, vk::ColorSpaceKHR::SRGB_NONLINEAR) {
                        Ok(fmt)
                } else {
                        Ok(formats[0])
                }
        }

        fn choose_present_mode(
                surface_loader: &Surface,
                surface: vk::SurfaceKHR,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::PresentModeKHR> {
                let modes =
                        unsafe { surface_loader.get_physical_device_surface_present_modes(physical_device, surface)? };

                let find_present_mode = |mode: vk::PresentModeKHR| modes.iter().any(|&m| m == mode);

                if find_present_mode(vk::PresentModeKHR::MAILBOX) {
                        Ok(vk::PresentModeKHR::MAILBOX)
                } else if find_present_mode(vk::PresentModeKHR::IMMEDIATE) {
                        Ok(vk::PresentModeKHR::IMMEDIATE)
                } else {
                        Ok(vk::PresentModeKHR::FIFO)
                }
        }
}





pub struct VkReusableCommandBuffer {
        handle: vk::CommandBuffer,
        fence:  vk::Fence,
}



impl VkReusableCommandBuffer {
        pub fn new(device: &ash::Device, cmd_pool: vk::CommandPool) -> VkResult<Self> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .command_buffer_count(1)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handle = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)?[0] };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
                let fence = unsafe { device.create_fence(&fence_cinfo, None)? };

                Ok(Self {
                        handle,
                        fence,
                })
        }

        pub fn new_array(device: &ash::Device, cmd_pool: vk::CommandPool, count: u32) -> VkResult<Vec<Self>> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .command_buffer_count(count)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handles = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

                handles.iter()
                        .map(|&handle| {
                                let fence = unsafe { device.create_fence(&fence_cinfo, None)? };

                                Ok(Self {
                                        handle,
                                        fence,
                                })
                        })
                        .collect()
        }

        pub fn record_and_submit<F>(
                &self,
                device: &ash::Device,
                submit_queue: vk::Queue,
                wait_semaphores: &[vk::Semaphore],
                wait_stages: &[vk::PipelineStageFlags],
                signal_semaphores: &[vk::Semaphore],
                f: F,
        ) -> VkResult<()>
        where
                F: FnOnce(&ash::Device, vk::CommandBuffer),
        {
                unsafe {
                        {
                                //let t = Timer::new("wait_for_fences took: ");

                                device.wait_for_fences(&[self.fence], true, u64::MAX)?;
                        }
                        device.reset_fences(&[self.fence])?;
                        device.reset_command_buffer(self.handle, vk::CommandBufferResetFlags::RELEASE_RESOURCES)?;

                        let cmd_buffer_binfo = vk::CommandBufferBeginInfo::builder()
                                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                        device.begin_command_buffer(self.handle, &cmd_buffer_binfo)?;
                        f(&device, self.handle);
                        device.end_command_buffer(self.handle)?;

                        let cmd_buffers = [self.handle];

                        let submit_info = vk::SubmitInfo::builder()
                                .command_buffers(&cmd_buffers)
                                .wait_semaphores(wait_semaphores)
                                .wait_dst_stage_mask(wait_stages)
                                .signal_semaphores(signal_semaphores);

                        device.queue_submit(submit_queue, &[submit_info.build()], self.fence)?;

                        Ok(())
                }
        }

        pub fn wait(&self, device: &ash::Device, timeout: u64) -> VkResult<()> {
                unsafe { device.wait_for_fences(&[self.fence], true, timeout) }
        }
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

fn create_shader_module<D: DeviceV1_0>(device: &D, path: &'static str) -> VkResult<vk::ShaderModule> {
        let compile_path = path.to_string() + ".spv";

        let mut child = Command::new("res/misc/glslc.exe")
                .arg(path)
                .arg("-o")
                .arg(&compile_path)
                .spawn()
                .expect("Failed to compile shaders!");

        let exit_status = child
                .wait()
                .expect("Error occurred while waiting for glslc.exe completion");

        if !exit_status.success() {
                error!(
                        "glslc.exe did not exit successfully: {}",
                        exit_status.code().unwrap_or(0)
                );
        }

        let shader_code = std::fs::read(&compile_path).expect("Failed to read shader binary file!");

        let mut shader_module_cinfo = vk::ShaderModuleCreateInfo::builder().build();
        shader_module_cinfo.code_size = shader_code.len();
        shader_module_cinfo.p_code = shader_code.as_ptr() as *const u32;

        assert_eq!(shader_code.len() % 4, 0, "Shader code is invalid!");

        unsafe { device.create_shader_module(&shader_module_cinfo, None) }
}
