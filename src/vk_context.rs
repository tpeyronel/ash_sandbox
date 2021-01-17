use std::{
        borrow::Cow,
        error::Error,
        ffi::{c_void, CStr, CString},
        os::raw::c_char,
        slice,
};

extern crate vk_mem as vma;

use std::{mem::size_of, ops::Deref, process::Command, rc::Rc, time::Instant};

use ash::{
        extensions::{
                ext::DebugUtils,
                khr::{Surface, Swapchain},
        },
        prelude::VkResult,
        version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
        vk,
        vk::Extent3D,
        Device, Instance,
};
use bitflags;
use log::{debug, error, info, trace, warn};
use winit::window::Window;

use crate::vk_image::{VkImage, VkImageCreateInfo};
use crate::vk_wrapper::{
        VkDebugUtilsMessenger, VkDevice, VkFramebuffer, VkImageView, VkInstance, VkSampler, VkSurface,
};
use crate::vkma_error::VkmaResult;
use crate::{
        image::Image,
        my_vec::*,
        renderer::Renderer,
        timer::Timer,
        vertex::Vertex,
        vk_buffer::{VkBuffer, VkBufferCreateInfo, VkImmutableBufferCreateInfo},
};
use std::sync::Arc;

macro_rules! cstring {
        ($s:expr) => {
                CString::new($s).unwrap()
        };
}

/*struct VkSwapchainOutdatedCauses {
        window_resize: bool,
        suboptimal:    bool,
        out_of_date:   bool,
}

enum VkSwapchainOutdatedState {
        No,
        Yes(VkSwapchainOutdatedCauses),
}*/

bitflags! {
        struct VkSwapchainOutdatedCauses: u32 {
                const NONE = 0b00000000;
                const WINDOW_RESIZE = 0b00000001;
                const SUBOPTIMAL = 0b00000010;
                const OUT_OF_DATE = 0b00000100;
        }
}

const DEPTH_STENCIL_FORMAT: vk::Format = vk::Format::D24_UNORM_S8_UINT;

pub struct VkContext {
        window: Rc<Window>,

        entry: Arc<ash::Entry>,
        instance: Arc<VkInstance>,

        debug_utils_messenger: Option<VkDebugUtilsMessenger>,

        surface: Arc<VkSurface>,

        physical_device: vk::PhysicalDevice,
        device: Arc<VkDevice>,

        q_family_i: VkQueueFamilyIndices,
        queues: VkQueues,

        allocator: Arc<vma::Allocator>,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauses,

        render_pass: vk::RenderPass,
        //imgui_render_pass: vk::RenderPass,
        cmd_pool: vk::CommandPool,
        setup_cmd_buffer: VkReusableCommandBuffer,
        draw_cmd_buffers: Vec<VkReusableCommandBuffer>,

        vertex_buffer: VkBuffer,
        index_buffer: VkBuffer,
        matrices_buffers: Vec<VkBuffer>,

        vk_img: VkImage,
        vk_img_view: VkImageView,
        vk_img_sampler: VkSampler,

        desc_pool: vk::DescriptorPool,
        desc_set_layout: vk::DescriptorSetLayout,
        desc_sets: Vec<vk::DescriptorSet>,

        graphics_pipeline_layout: vk::PipelineLayout,
        graphics_pipeline: vk::Pipeline,
        viewport: vk::Viewport,
        scissor: vk::Rect2D,

        img_avail_semaphores: Vec<vk::Semaphore>,
        present_complete_semaphores: Vec<vk::Semaphore>,

        imgui_renderer: Option<imgui_rs_vulkan_renderer::Renderer>,

        creation_instant: Instant,
        frame_i: usize,
        frame_counter: u32,
        recreate_swapchain: RecreateSwapchain,
}

#[cfg(all(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = true;
#[cfg(not(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = false;

impl VkContext {
        pub fn new(window: &Rc<Window>, imgui_context: &mut imgui::Context) -> Result<Self, Box<dyn Error>> {
                let _t = Timer::new("Initialized VkContext in: ");

                let entry = Arc::new(ash::Entry::new()?);

                let instance = Self::create_instance(window, &entry)?;
                trace!("Created VkInstance");

                let debug_utils_messenger = if !ENABLE_VALIDATION_LAYERS {
                        None
                } else {
                        let debug_messenger = Some(Self::create_debug_utils_messenger(&entry, &instance)?);

                        trace!("Created VkDebugUtilsMessenger");

                        debug_messenger
                };

                let surface = Arc::new(unsafe { VkSurface::new(&entry, &instance, window)? });
                trace!("Created VkSurface");

                let (physical_device, q_family_i) = Self::choose_physical_device(&instance, &surface)?;

                trace!("Chose VkPhysicalDevice");
                info!("Chosen physical device: {:?}", unsafe {
                        CStr::from_ptr(
                                instance.get_physical_device_properties(physical_device)
                                        .device_name
                                        .as_ptr(),
                        )
                });
                info!("Queue family indices: {:?}", &q_family_i);

                let (device, queues) = Self::create_device(&instance, physical_device, &q_family_i)?;
                trace!("Created VkDevice");

                let allocator = Self::create_allocator(&instance, physical_device, &device)?;
                trace!("Created VmaAllocator");

                let mut swapchain = VkSwapchain::new(
                        window,
                        &instance,
                        &surface,
                        physical_device,
                        &device,
                        &allocator,
                        vk::SwapchainKHR::null(),
                )?;
                trace!("Created VkSwapchain");

                let (viewport, scissor) = Self::create_viewport_and_scissor(swapchain.extent);

                let render_pass = Self::create_render_pass(&device, swapchain.format.format)?;
                trace!("Created VkRenderPass");
                //let imgui_render_pass = Self::create_imgui_render_pass(&device, swapchain.format.format)?;

                swapchain.create_framebuffers(&device, render_pass)?;
                trace!("Created VkFramebuffers");

                let cmd_pool = Self::create_command_pool(&device, &q_family_i)?;
                trace!("Created VkCommandPool");

                let setup_cmd_buffer = VkReusableCommandBuffer::new(&device, cmd_pool)?;
                let draw_cmd_buffers = VkReusableCommandBuffer::new_vec(&device, cmd_pool, swapchain.img_count)?;
                trace!("Allocated VkCommandBuffers");

                let vertex_buffer = Self::create_vertex_buffer(&device, &allocator, &queues, &setup_cmd_buffer)?;
                trace!("Created vertex buffer");
                let index_buffer = Self::create_index_buffer(&device, &allocator, &queues, &setup_cmd_buffer)?;
                trace!("Created index buffer");
                let matrices_buffers = Self::create_matrices_buffers(&device, &allocator, swapchain.img_count)?;
                trace!("Created matrices uniform buffer");

                let (vk_img, vk_img_view, vk_img_sampler) = Self::create_texture_image(
                        unsafe { &instance.get_physical_device_properties(physical_device).limits },
                        &device,
                        &allocator,
                        &setup_cmd_buffer,
                        queues.graphics,
                )?;
                trace!("Created VkImage");

                let desc_pool = Self::create_descriptor_pool(&device)?;
                let desc_set_layout = Self::create_descriptor_set_layout(&device)?;
                let desc_sets = Self::create_descriptor_sets(
                        &device,
                        desc_pool,
                        desc_set_layout,
                        &matrices_buffers,
                        *vk_img_view,
                        *vk_img_sampler,
                )?;
                trace!("Created VkDescriptorSets");

                let graphics_pipeline_layout = Self::create_graphics_pipeline_layout(&device, desc_set_layout)?;
                trace!("Created VkGraphicsPipelineLayout");

                let graphics_pipeline = Self::create_graphics_pipeline(
                        &device,
                        &viewport,
                        &scissor,
                        render_pass,
                        graphics_pipeline_layout,
                )?;
                trace!("Created VkGraphicsPipeline");

                let (img_avail_semaphores, present_complete_semaphores) =
                        Self::create_sync_objects(&device, swapchain.img_count)?;
                trace!("Created VkSemaphores");

                let mut s = Self {
                        window: Rc::clone(window),

                        entry,
                        instance,

                        debug_utils_messenger,

                        surface,

                        physical_device,
                        device,

                        q_family_i,
                        queues,

                        allocator,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauses::NONE,

                        render_pass,
                        //imgui_render_pass,
                        cmd_pool,
                        setup_cmd_buffer,
                        draw_cmd_buffers,

                        vertex_buffer,
                        index_buffer,
                        matrices_buffers,

                        vk_img,
                        vk_img_view,
                        vk_img_sampler,

                        desc_pool,
                        desc_set_layout,
                        desc_sets,

                        graphics_pipeline_layout,
                        graphics_pipeline,
                        viewport,
                        scissor,

                        img_avail_semaphores,
                        present_complete_semaphores,

                        imgui_renderer: None,

                        creation_instant: Instant::now(),
                        frame_i: 0,
                        frame_counter: 0,
                        recreate_swapchain: RecreateSwapchain::No,
                };

                s.imgui_renderer = Some(imgui_rs_vulkan_renderer::Renderer::new(
                        &s,
                        s.swapchain.img_count as usize,
                        s.render_pass,
                        imgui_context,
                )?);

                Ok(s)
        }

        pub fn on_window_resize(&mut self, width: u32, height: u32) {
                /*if width == 0 || height == 0 {
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
                }*/

                self.swapchain_outdated_causes
                        .insert(VkSwapchainOutdatedCauses::WINDOW_RESIZE);
        }

        fn recreate_swapchain(&mut self) -> Result<(), Box<dyn Error>> {
                // If resize is the only cause, then check that we actually need to resize
                if self.swapchain_outdated_causes == VkSwapchainOutdatedCauses::WINDOW_RESIZE {
                        let window_size = self.window.inner_size();

                        if window_size.width == self.swapchain.extent.width
                                && window_size.height == self.swapchain.extent.height
                        {
                                self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::NONE;

                                return Ok(());
                        }
                }

                trace!("Recreating VkSwapchain");
                let _t = Timer::new("Recreated VkSwapchain in: ");

                let mut recreate_pipeline = self
                        .swapchain_outdated_causes
                        .contains(VkSwapchainOutdatedCauses::OUT_OF_DATE);

                unsafe { self.device.device_wait_idle()? };

                let old_swapchain = {
                        let old_swapchain_handle = self.swapchain.handle;

                        std::mem::replace(
                                &mut self.swapchain,
                                VkSwapchain::new(
                                        &self.window,
                                        &self.instance,
                                        &self.surface,
                                        self.physical_device,
                                        &self.device,
                                        &self.allocator,
                                        old_swapchain_handle,
                                )?,
                        )
                };

                let (viewport, scissor) = Self::create_viewport_and_scissor(self.swapchain.extent);
                self.viewport = viewport;
                self.scissor = scissor;

                if old_swapchain.format != self.swapchain.format {
                        unsafe { self.device.destroy_render_pass(self.render_pass, None) };
                        self.render_pass = Self::create_render_pass(&self.device, self.swapchain.format.format)?;

                        /*unsafe { self.device.destroy_render_pass(self.imgui_render_pass, None) };
                        self.imgui_render_pass =
                                Self::create_imgui_render_pass(&self.device, self.swapchain.format.format)?;*/

                        let mut imgui_renderer = self.imgui_renderer.take().unwrap();
                        imgui_renderer.set_render_pass(self, self.render_pass)?;
                        self.imgui_renderer = Some(imgui_renderer);

                        recreate_pipeline = true;
                }

                self.swapchain.create_framebuffers(&self.device, self.render_pass)?;

                if old_swapchain.img_count != self.swapchain.img_count {
                        self.matrices_buffers =
                                Self::create_matrices_buffers(&self.device, &self.allocator, self.swapchain.img_count)?;

                        unsafe { self.device.free_descriptor_sets(self.desc_pool, &self.desc_sets) };
                        self.desc_sets = Self::create_descriptor_sets(
                                &self.device,
                                self.desc_pool,
                                self.desc_set_layout,
                                &self.matrices_buffers,
                                *self.vk_img_view,
                                *self.vk_img_sampler,
                        )?;

                        for cmd_buffer in &self.draw_cmd_buffers {
                                unsafe { self.device.free_command_buffers(self.cmd_pool, &[cmd_buffer.handle]) };
                        }
                        self.draw_cmd_buffers = VkReusableCommandBuffer::new_vec(
                                &self.device,
                                self.cmd_pool,
                                self.swapchain.img_count,
                        )?;

                        for &semaphore in &self.img_avail_semaphores {
                                unsafe { self.device.destroy_semaphore(semaphore, None) };
                        }
                        for &semaphore in &self.present_complete_semaphores {
                                unsafe { self.device.destroy_semaphore(semaphore, None) };
                        }
                        let (img_avail_semaphores, present_complete_semaphores) =
                                Self::create_sync_objects(&self.device, self.swapchain.img_count)?;
                        self.img_avail_semaphores = img_avail_semaphores;
                        self.present_complete_semaphores = present_complete_semaphores;

                        self.frame_i = 0;
                }

                if recreate_pipeline {
                        unsafe { self.device.destroy_pipeline(self.graphics_pipeline, None) };
                        self.graphics_pipeline = Self::create_graphics_pipeline(
                                &self.device,
                                &self.viewport,
                                &self.scissor,
                                self.render_pass,
                                self.graphics_pipeline_layout,
                        )?;
                }

                self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::NONE;

                Ok(())
        }

        fn update_matrices_buffer(&self, buffer: &VkBuffer) -> vma::Result<()> {
                let time = self.creation_instant.elapsed().as_secs_f32();

                let data = Matrices3D {
                        model: glm::rotate(&Mat4::identity(), time, &Vec3::new(0.0, 1.0, 0.0)),
                        view: glm::look_at_lh(&Vec3::new(0.0, time.sin(), -1.0), &Vec3::new(0.0, 0.0, 0.0), &Vec3::y()),
                        proj: glm::perspective_fov_lh_zo(
                                90.0f32.to_radians(),
                                self.swapchain.extent.width as f32,
                                self.swapchain.extent.height as f32,
                                0.1,
                                100.0,
                        ),
                };

                let buffer_size = std::mem::size_of::<Matrices3D>() as vk::DeviceSize;

                let map = buffer.map_memory(&self.allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(&data as *const _ as *const u8, map, buffer_size as usize);
                }
                buffer.unmap_memory(&self.allocator)?;

                Ok(())
        }

        fn create_instance(window: &Window, entry: &Arc<ash::Entry>) -> Result<Arc<VkInstance>, Box<dyn Error>> {
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
                        req_extensions.push(cstring!("VK_KHR_get_physical_device_properties2"));

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

                        Ok(Arc::new(VkInstance::new(entry, &instance_cinfo)?))
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
                entry: &ash::Entry,
                instance: &ash::Instance,
        ) -> VkResult<VkDebugUtilsMessenger> {
                let debug_cinfo = Self::create_debug_utils_messenger_cinfo();

                unsafe { VkDebugUtilsMessenger::new(entry, instance, &debug_cinfo) }
        }

        fn choose_physical_device(
                instance: &ash::Instance,
                surface: &VkSurface,
        ) -> Result<(vk::PhysicalDevice, VkQueueFamilyIndices), Box<dyn Error>> {
                Ok(unsafe {
                        let ph_devices = instance.enumerate_physical_devices()?;

                        ph_devices
                                .iter()
                                .filter_map(|&pd| Self::is_device_suitable(instance, surface, pd))
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
                surface: &VkSurface,
                pd: vk::PhysicalDevice,
        ) -> Option<(vk::PhysicalDevice, VkQueueFamilyIndices)> {
                let q_family_i = match VkQueueFamilyIndices::new(instance, surface, pd) {
                        Some(q_family_i) => q_family_i,
                        None => return None,
                };

                let supported_features = unsafe { instance.get_physical_device_features(pd) };

                if supported_features.sampler_anisotropy == vk::FALSE {
                        return None;
                }

                Some((pd, q_family_i))
        }

        fn create_device(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                q_family_i: &VkQueueFamilyIndices,
        ) -> Result<(Arc<VkDevice>, VkQueues), Box<dyn Error>> {
                let memory_budget_ext = CStr::from_bytes_with_nul(b"VK_EXT_memory_budget\0").unwrap();

                let req_device_extensions_raw = vec![Swapchain::name().as_ptr(), memory_budget_ext.as_ptr()];
                let req_device_features = vk::PhysicalDeviceFeatures::builder()
                        .sampler_anisotropy(true)
                        .shader_clip_distance(true);

                let queue_priorities;

                let device_q_cinfos = if q_family_i.graphics == q_family_i.present {
                        queue_priorities = vec![1.0];

                        vec![vk::DeviceQueueCreateInfo::builder()
                                .queue_family_index(q_family_i.graphics)
                                .queue_priorities(&queue_priorities)
                                .build()]
                } else {
                        queue_priorities = vec![0.75, 0.25];

                        vec![
                                vk::DeviceQueueCreateInfo::builder()
                                        .queue_family_index(q_family_i.graphics)
                                        .queue_priorities(&queue_priorities[0..1])
                                        .build(),
                                vk::DeviceQueueCreateInfo::builder()
                                        .queue_family_index(q_family_i.present)
                                        .queue_priorities(&queue_priorities[1..2])
                                        .build(),
                        ]
                };

                let device_cinfo = vk::DeviceCreateInfo::builder()
                        .queue_create_infos(&device_q_cinfos)
                        .enabled_extension_names(&req_device_extensions_raw)
                        .enabled_features(&req_device_features);

                let device = unsafe { Arc::new(VkDevice::new(&instance, physical_device, &device_cinfo)?) };

                let queues = VkQueues {
                        graphics: unsafe { device.get_device_queue(q_family_i.graphics, 0) },
                        present: unsafe { device.get_device_queue(q_family_i.present, 0) },
                };

                Ok((device, queues))
        }

        fn create_allocator(
                instance: &ash::Instance,
                physical_device: vk::PhysicalDevice,
                device: &ash::Device,
        ) -> vma::Result<Arc<vma::Allocator>> {
                let allocator_cinfo = vma::AllocatorCreateInfo {
                        physical_device,
                        device: device.clone(),
                        instance: instance.clone(),
                        flags: vma::AllocatorCreateFlags::NONE,
                        preferred_large_heap_block_size: 0,
                        frame_in_use_count: 0,
                        heap_size_limits: None,
                };

                Ok(Arc::new(vma::Allocator::new(&allocator_cinfo)?))
        }

        fn create_vertex_buffer(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkBuffer, Box<dyn Error>> {
                let data = [
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, 0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                        //
                        //
                        //
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(0.0, 1.0),
                        },
                        Vertex {
                                pos: Vec3::new(0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(0.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, 0.5),
                                tex_coord: Vec2::new(1.0, 0.0),
                        },
                        Vertex {
                                pos: Vec3::new(-0.5, -0.5, -0.5),
                                tex_coord: Vec2::new(1.0, 1.0),
                        },
                ];

                let cinfo = VkImmutableBufferCreateInfo {
                        device,
                        allocator,
                        cmd_buffer: setup_cmd_buffer,
                        transfer_queue: queues.graphics,
                        buffer_usage: vk::BufferUsageFlags::VERTEX_BUFFER,
                        data: &data,
                };

                VkBuffer::new_immutable(&cinfo)

                /*let buffer_size = (std::mem::size_of::<Vertex>() * data.len()) as vk::DeviceSize;

                let cinfo = VkBufferCreateInfo {
                        device,
                        allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::VERTEX_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                        pref_mem_flags: Default::default(),
                        q_family_indices: None,
                };

                let buffer = VkBuffer::new(&cinfo)?;

                let map = buffer.map_memory(allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, map, buffer_size as usize);
                }
                buffer.unmap_memory(allocator)?;

                Ok(buffer)*/
        }

        fn create_index_buffer(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                queues: &VkQueues,
                setup_cmd_buffer: &VkReusableCommandBuffer,
        ) -> Result<VkBuffer, Box<dyn Error>> {
                let data: [u32; 36] = [
                        0, 1, 2, 2, 3, 0, //
                        4, 5, 6, 6, 7, 4, //
                        8, 9, 10, 10, 11, 8, //
                        12, 13, 14, 14, 15, 12, //
                        16, 17, 18, 18, 19, 16, //
                        20, 21, 22, 22, 23, 20, //
                ];

                let cinfo = VkImmutableBufferCreateInfo {
                        device,
                        allocator,
                        cmd_buffer: setup_cmd_buffer,
                        transfer_queue: queues.graphics,
                        buffer_usage: vk::BufferUsageFlags::INDEX_BUFFER,
                        data: &data,
                };

                VkBuffer::new_immutable(&cinfo)

                /*let buffer_size = (std::mem::size_of::<u32>() * data.len()) as vk::DeviceSize;

                let cinfo = VkBufferCreateInfo {
                        device,
                        allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::INDEX_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                        pref_mem_flags: Default::default(),
                        q_family_indices: None,
                };

                let buffer = VkBuffer::new(&cinfo)?;

                let map = buffer.map_memory(allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, map, buffer_size as usize);
                }
                buffer.unmap_memory(allocator)?;

                Ok(buffer)*/
        }

        fn create_matrices_buffers(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                swch_img_count: u32,
        ) -> Result<Vec<VkBuffer>, Box<dyn Error>> {
                let buffer_size = std::mem::size_of::<Matrices3D>() as vk::DeviceSize;

                let cinfo = VkBufferCreateInfo {
                        device,
                        allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                let mut buffers = Vec::with_capacity(swch_img_count as usize);

                for _ in 0..swch_img_count {
                        buffers.push(VkBuffer::new(&cinfo)?);
                }

                Ok(buffers)
        }

        fn create_texture_image(
                pd_limits: &vk::PhysicalDeviceLimits,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                cmd_buffer: &VkReusableCommandBuffer,
                transfer_queue: vk::Queue,
        ) -> Result<(VkImage, VkImageView, VkSampler), Box<dyn Error>> {
                unsafe { stb_image::stb_image::bindgen::stbi_set_flip_vertically_on_load(1) };

                let img = Image::new(const_cstr!("res/tex/wall.jpg").as_cstr(), 4)?;

                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,
                                buffer_size: img.data_size() as vk::DeviceSize,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuToGpu,
                                alloc_flags: vma::AllocationCreateFlags::NONE,
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(&buffer_cinfo)?
                };

                let buffer_data = staging_buffer.map_memory(&allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(img.data(), buffer_data, img.data_size());
                }
                staging_buffer.unmap_memory(&allocator)?;
                staging_buffer.flush_memory(&allocator)?;

                let vk_img_cinfo = VkImageCreateInfo {
                        image_type: vk::ImageType::TYPE_2D,
                        format: vk::Format::R8G8B8A8_SRGB,
                        extent: vk::Extent3D {
                                width: img.width(),
                                height: img.height(),
                                depth: 1,
                        },
                        mip_levels: 1,
                        array_layers: 1,
                        samples: vk::SampleCountFlags::TYPE_1,
                        tiling: vk::ImageTiling::OPTIMAL,
                        usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_cflags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags: Default::default(),
                };

                let vk_img = VkImage::new(allocator, &vk_img_cinfo)?;

                /*let (vk_img, vk_img_alloc, _) = {
                        let vk_img_cinfo = vk::ImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format: vk::Format::R8G8B8A8_SRGB,
                                extent: vk::Extent3D {
                                        width: img.width(),
                                        height: img.height(),
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples: vk::SampleCountFlags::TYPE_1,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                                sharing_mode: vk::SharingMode::EXCLUSIVE,
                                queue_family_index_count: 0,
                                p_queue_family_indices: std::ptr::null(),
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                ..vk::ImageCreateInfo::default()
                        };

                        let alloc_cinfo = vma::AllocationCreateInfo {
                                usage: vma::MemoryUsage::GpuOnly,
                                flags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                                memory_type_bits: 0,
                                pool: None,
                                user_data: None,
                        };

                        allocator.create_image(&vk_img_cinfo, &alloc_cinfo)?
                };*/

                cmd_buffer.record_and_submit(&device, transfer_queue, &[], &[], &[], |device, cmd_buffer| {
                        let barrier = vk::ImageMemoryBarrier {
                                src_access_mask: vk::AccessFlags::empty(),
                                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: *vk_img,
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageMemoryBarrier::default()
                        };

                        unsafe {
                                device.cmd_pipeline_barrier(
                                        cmd_buffer,
                                        vk::PipelineStageFlags::TOP_OF_PIPE,
                                        vk::PipelineStageFlags::TRANSFER,
                                        vk::DependencyFlags::empty(),
                                        &[],
                                        &[],
                                        &[barrier],
                                )
                        };

                        let region = vk::BufferImageCopy {
                                buffer_offset: 0,
                                buffer_row_length: 0,
                                buffer_image_height: 0,
                                image_subresource: vk::ImageSubresourceLayers {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        mip_level: 0,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                                image_extent: vk::Extent3D {
                                        width: img.width(),
                                        height: img.height(),
                                        depth: 1,
                                },
                        };

                        unsafe {
                                device.cmd_copy_buffer_to_image(
                                        cmd_buffer,
                                        *staging_buffer,
                                        *vk_img,
                                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        &[region],
                                )
                        };

                        let barrier = vk::ImageMemoryBarrier {
                                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                dst_access_mask: vk::AccessFlags::SHADER_READ,
                                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                image: *vk_img,
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageMemoryBarrier::default()
                        };

                        unsafe {
                                device.cmd_pipeline_barrier(
                                        cmd_buffer,
                                        vk::PipelineStageFlags::TRANSFER,
                                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                                        vk::DependencyFlags::empty(),
                                        &[],
                                        &[],
                                        &[barrier],
                                )
                        };

                        Ok(())
                })?;

                let vk_img_view = unsafe {
                        let vk_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *vk_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk::Format::R8G8B8A8_SRGB,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &vk_img_view_cinfo)?
                };

                cmd_buffer.wait(&device, u64::MAX)?;

                let vk_img_sampler = unsafe {
                        let sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: vk::Filter::LINEAR,
                                min_filter: vk::Filter::LINEAR,
                                address_mode_u: vk::SamplerAddressMode::REPEAT,
                                address_mode_v: vk::SamplerAddressMode::REPEAT,
                                address_mode_w: vk::SamplerAddressMode::REPEAT,
                                anisotropy_enable: vk::TRUE,
                                max_anisotropy: pd_limits.max_sampler_anisotropy,
                                compare_enable: 0,
                                compare_op: vk::CompareOp::ALWAYS,
                                mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                                mip_lod_bias: 0.0,
                                min_lod: 0.0,
                                max_lod: 0.0,
                                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                                unnormalized_coordinates: vk::FALSE,
                                ..vk::SamplerCreateInfo::default()
                        };

                        VkSampler::new(device, &sampler_cinfo)?
                };

                Ok((vk_img, vk_img_view, vk_img_sampler))
        }

        fn create_descriptor_pool(device: &ash::Device) -> VkResult<vk::DescriptorPool> {
                let pool_sizes = [
                        vk::DescriptorPoolSize {
                                ty: vk::DescriptorType::UNIFORM_BUFFER,
                                descriptor_count: 100,
                        },
                        vk::DescriptorPoolSize {
                                ty: vk::DescriptorType::SAMPLED_IMAGE,
                                descriptor_count: 100,
                        },
                ];

                let desc_pool_cinfo = vk::DescriptorPoolCreateInfo::builder()
                        .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                        .pool_sizes(&pool_sizes)
                        .max_sets(1000);

                unsafe { device.create_descriptor_pool(&desc_pool_cinfo, None) }
        }

        fn create_descriptor_set_layout(device: &ash::Device) -> VkResult<vk::DescriptorSetLayout> {
                let mat_binding = vk::DescriptorSetLayoutBinding {
                        binding: 0,
                        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::VERTEX,
                        p_immutable_samplers: std::ptr::null(),
                };

                let tex_binding = vk::DescriptorSetLayoutBinding {
                        binding: 1,
                        descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                        p_immutable_samplers: std::ptr::null(),
                };

                let bindings = [mat_binding, tex_binding];

                let desc_set_layout_cinfo = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

                unsafe { device.create_descriptor_set_layout(&desc_set_layout_cinfo, None) }
        }

        fn create_descriptor_sets(
                device: &ash::Device,
                desc_pool: vk::DescriptorPool,
                desc_layout: vk::DescriptorSetLayout,
                matrices_buffers: &[VkBuffer],
                img_view: vk::ImageView,
                sampler: vk::Sampler,
        ) -> VkResult<Vec<vk::DescriptorSet>> {
                let desc_set_layouts = vec![desc_layout; matrices_buffers.len()];

                let desc_set_ainfo = vk::DescriptorSetAllocateInfo::builder()
                        .descriptor_pool(desc_pool)
                        .set_layouts(&desc_set_layouts);

                let desc_sets = unsafe { device.allocate_descriptor_sets(&desc_set_ainfo)? };

                assert_eq!(matrices_buffers.len(), desc_sets.len());

                for (matrices_buffer, &desc_set) in matrices_buffers.iter().zip(desc_sets.iter()) {
                        let buffer_info = vk::DescriptorBufferInfo {
                                buffer: **matrices_buffer,
                                offset: 0,
                                range: size_of::<Matrices3D>() as vk::DeviceSize,
                        };

                        let mat_desc_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                                .dst_set(desc_set)
                                .dst_binding(0)
                                .dst_array_element(0)
                                .buffer_info(std::slice::from_ref(&buffer_info))
                                .build();

                        let sampler_info = vk::DescriptorImageInfo {
                                sampler,
                                image_view: img_view,
                                image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        };

                        let sampler_desc_write = vk::WriteDescriptorSet::builder()
                                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                                .dst_set(desc_set)
                                .dst_binding(1)
                                .dst_array_element(0)
                                .image_info(std::slice::from_ref(&sampler_info))
                                .build();

                        let writes = [mat_desc_write, sampler_desc_write];

                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                }

                Ok(desc_sets)
        }

        fn create_render_pass(device: &ash::Device, swapchain_format: vk::Format) -> VkResult<vk::RenderPass> {
                let attachments = [
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swapchain_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: DEPTH_STENCIL_FORMAT,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                        },
                ];

                let color_attachment_ref = vk::AttachmentReference {
                        attachment: 0,
                        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                };

                let depth_attachment_ref = vk::AttachmentReference {
                        attachment: 1,
                        layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                };

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        .color_attachments(slice::from_ref(&color_attachment_ref))
                        .depth_stencil_attachment(&depth_attachment_ref)
                        //.input_attachments(&[])
                        //.resolve_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];

                let subpass_dependencies = [vk::SubpassDependency {
                        src_subpass: vk::SUBPASS_EXTERNAL,
                        dst_subpass: 0,
                        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                        dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                        dependency_flags: vk::DependencyFlags::empty(),
                }];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { device.create_render_pass(&render_pass_cinfo, None) }
        }

        /*fn create_imgui_render_pass(device: &ash::Device, swapchain_format: vk::Format) -> VkResult<vk::RenderPass> {
                let attachments = [
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swapchain_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::LOAD,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: DEPTH_STENCIL_FORMAT,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::LOAD,
                                store_op: vk::AttachmentStoreOp::DONT_CARE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                        },
                ];

                let color_attachment_ref = vk::AttachmentReference {
                        attachment: 0,
                        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                };

                let depth_attachment_ref = vk::AttachmentReference {
                        attachment: 1,
                        layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                };

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        .color_attachments(slice::from_ref(&color_attachment_ref))
                        .depth_stencil_attachment(&depth_attachment_ref)
                        //.input_attachments(&[])
                        //.resolve_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];

                let subpass_dependencies = [vk::SubpassDependency {
                        src_subpass: vk::SUBPASS_EXTERNAL,
                        dst_subpass: 0,
                        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                        dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                        dependency_flags: vk::DependencyFlags::empty(),
                }];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { device.create_render_pass(&render_pass_cinfo, None) }
        }*/

        fn create_viewport_and_scissor(swapchain_extent: vk::Extent2D) -> (vk::Viewport, vk::Rect2D) {
                (
                        vk::Viewport {
                                x: 0.0,
                                y: swapchain_extent.height as f32,
                                width: swapchain_extent.width as f32,
                                height: -(swapchain_extent.height as f32),
                                min_depth: 0.0,
                                max_depth: 1.0,
                        },
                        vk::Rect2D {
                                offset: vk::Offset2D { x: 0, y: 0 },
                                extent: swapchain_extent,
                        },
                )
        }

        fn create_graphics_pipeline_layout(
                device: &ash::Device,
                desc_set_layout: vk::DescriptorSetLayout,
        ) -> VkResult<vk::PipelineLayout> {
                let layout_cinfo = vk::PipelineLayoutCreateInfo::builder()
                        /*.push_constant_ranges(&[])*/
                        .set_layouts(std::slice::from_ref(&desc_set_layout));

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

                let viewport_state_cinfo = vk::PipelineViewportStateCreateInfo::builder()
                        .viewports(slice::from_ref(&viewport))
                        .scissors(slice::from_ref(&scissor));

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

                let depth_stencil_state_cinfo = vk::PipelineDepthStencilStateCreateInfo::builder()
                        .depth_test_enable(true)
                        .depth_write_enable(true)
                        .depth_compare_op(vk::CompareOp::LESS)
                        .depth_bounds_test_enable(false)
                        .stencil_test_enable(false)
                        .build();

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
                        .depth_stencil_state(&depth_stencil_state_cinfo)
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
                        }
                        Err((graphics_pipelines, err)) => {
                                assert_eq!(graphics_pipelines.len(), 1);

                                Err(err)
                        }
                }
        }

        fn create_command_pool(device: &ash::Device, q_family_i: &VkQueueFamilyIndices) -> VkResult<vk::CommandPool> {
                let cmd_pool_cinfo = vk::CommandPoolCreateInfo::builder()
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                        .queue_family_index(q_family_i.graphics);

                unsafe { device.create_command_pool(&cmd_pool_cinfo, None) }
        }

        fn create_sync_objects(
                device: &ash::Device,
                swch_img_count: u32,
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
                        create_n_semaphores(device, &semaphore_cinfo, swch_img_count as usize)?,
                        create_n_semaphores(device, &semaphore_cinfo, swch_img_count as usize)?,
                ))
        }
}

impl Renderer for VkContext {
        fn draw(&mut self, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>> {
                let window_size = self.window.inner_size();
                if window_size.width == 0 || window_size.height == 0 {
                        return Ok(());
                }

                if self.swapchain_outdated_causes != VkSwapchainOutdatedCauses::NONE {
                        self.recreate_swapchain()?;
                }

                /*match self.recreate_swapchain {
                        RecreateSwapchain::No => (),
                        RecreateSwapchain::Swapchain => unsafe {
                                self.recreate_swapchain()?;
                                self.recreate_swapchain = RecreateSwapchain::No;
                        },
                        RecreateSwapchain::SwapchainAndPipeline => unsafe {
                                self.recreate_swapchain_and_pipeline()?;
                                self.recreate_swapchain = RecreateSwapchain::No;
                        },
                };*/

                let frame_img_avail_semaphore = self.img_avail_semaphores[self.frame_i];

                let (img_i, suboptimal) = unsafe {
                        self.swapchain
                                .acquire_next_image(u64::MAX, frame_img_avail_semaphore, vk::Fence::null())?
                };

                if suboptimal {
                        self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::SUBOPTIMAL;
                }

                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.frame_i];
                let frame_present_complete_semaphore = self.present_complete_semaphores[self.frame_i];

                let frame_framebuffer = &self.swapchain.framebuffers[img_i as usize];

                let time = self.creation_instant.elapsed().as_secs_f32();
                let intensity = ((time.sin() + 1.0) / 2.0) * 0.5;

                let clear_values = [
                        vk::ClearValue {
                                color: vk::ClearColorValue {
                                        float32: [intensity, intensity, intensity, 1.0],
                                },
                        },
                        vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                        },
                ];

                self.update_matrices_buffer(&self.matrices_buffers[img_i as usize])?;

                let mut imgui_renderer = self.imgui_renderer.take().unwrap();

                frame_draw_cmd_buffer.record_and_submit(
                        &self.device,
                        self.queues.graphics,
                        &[frame_img_avail_semaphore],
                        &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
                        &[frame_present_complete_semaphore],
                        |device, draw_cmd_buffer| unsafe {
                                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                        .render_pass(self.render_pass)
                                        .framebuffer(**frame_framebuffer)
                                        .render_area(self.scissor)
                                        .clear_values(&clear_values);

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

                                device.cmd_set_viewport(draw_cmd_buffer, 0, slice::from_ref(&self.viewport));
                                device.cmd_set_scissor(draw_cmd_buffer, 0, slice::from_ref(&self.scissor));

                                device.cmd_bind_vertex_buffers(draw_cmd_buffer, 0, &[*self.vertex_buffer], &[0]);
                                device.cmd_bind_index_buffer(
                                        draw_cmd_buffer,
                                        *self.index_buffer,
                                        0,
                                        vk::IndexType::UINT32,
                                );
                                device.cmd_bind_descriptor_sets(
                                        draw_cmd_buffer,
                                        vk::PipelineBindPoint::GRAPHICS,
                                        self.graphics_pipeline_layout,
                                        0,
                                        &[self.desc_sets[img_i as usize]],
                                        &[],
                                );

                                for _ in 0..1 {
                                        device.cmd_draw_indexed(draw_cmd_buffer, 36, 1, 0, 0, 0);
                                }

                                /*device.cmd_end_render_pass(draw_cmd_buffer);

                                let imgui_render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                        .render_pass(self.imgui_render_pass)
                                        .framebuffer(**frame_framebuffer)
                                        .render_area(self.scissor);

                                device.cmd_begin_render_pass(
                                        draw_cmd_buffer,
                                        &imgui_render_pass_binfo,
                                        vk::SubpassContents::INLINE,
                                );*/
                                imgui_renderer.cmd_draw(self, draw_cmd_buffer, imgui_draw_data)?;
                                device.cmd_end_render_pass(draw_cmd_buffer);

                                Ok(())
                        },
                )?;

                self.imgui_renderer = Some(imgui_renderer);

                unsafe {
                        match self.swapchain.queue_present(
                                self.queues.present,
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
                                }
                                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                        self.recreate_swapchain = RecreateSwapchain::SwapchainAndPipeline;
                                }
                                Err(err) => return Err(err.into()),
                                _ => {}
                        };
                }

                self.frame_i = (self.frame_i + 1) % (self.swapchain.img_count as usize);
                self.frame_counter += 1;

                Ok(())
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

                        self.device.destroy_descriptor_set_layout(self.desc_set_layout, None);
                        self.device.destroy_descriptor_pool(self.desc_pool, None);

                        self.device.destroy_render_pass(self.render_pass, None);
                        //self.device.destroy_render_pass(self.imgui_render_pass, None);
                }
        }
}

impl imgui_rs_vulkan_renderer::RendererVkContext for VkContext {
        fn instance(&self) -> &Instance {
                &self.instance
        }

        fn physical_device(&self) -> vk::PhysicalDevice {
                self.physical_device
        }

        fn device(&self) -> &Device {
                &self.device
        }

        fn queue(&self) -> vk::Queue {
                self.queues.graphics
        }

        fn command_pool(&self) -> vk::CommandPool {
                self.cmd_pool
        }
}

enum RecreateSwapchain {
        No,
        Swapchain,
        SwapchainAndPipeline,
}

#[derive(Debug)]
struct VkQueueFamilyIndices {
        graphics: u32,
        present: u32,
}

impl VkQueueFamilyIndices {
        fn new(instance: &ash::Instance, surface: &VkSurface, pd: vk::PhysicalDevice) -> Option<Self> {
                let q_families_props = unsafe { instance.get_physical_device_queue_family_properties(pd) };

                fn find_queue_family<F>(q_families_props: &[vk::QueueFamilyProperties], cond: F) -> Option<u32>
                where
                        F: Fn(usize, &vk::QueueFamilyProperties) -> bool,
                {
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
                        surface.loader()
                                .get_physical_device_surface_support(pd, i as u32, **surface)
                                .unwrap()
                };

                let supports_both = |i: usize, q_fam_props: &vk::QueueFamilyProperties| {
                        supports_graphics(i, q_fam_props) && supports_present(i, q_fam_props)
                };

                let graphics_and_present = find_queue_family(&q_families_props, supports_both);

                if let Some(graphics_and_present) = graphics_and_present {
                        return Some(Self {
                                graphics: graphics_and_present,
                                present: graphics_and_present,
                        });
                }

                let graphics = find_queue_family(&q_families_props, supports_graphics);
                let present = find_queue_family(&q_families_props, supports_present);

                if let (Some(graphics), Some(present)) = (graphics, present) {
                        return Some(Self { graphics, present });
                }

                None
        }
}

struct VkQueues {
        graphics: vk::Queue,
        present: vk::Queue,
}

struct VkSwapchain {
        loader: Swapchain,
        surface: Arc<VkSurface>,

        handle: vk::SwapchainKHR,
        format: vk::SurfaceFormatKHR,
        extent: vk::Extent2D,
        present_mode: vk::PresentModeKHR,
        color_imgs: Vec<vk::Image>,
        color_img_views: Vec<VkImageView>,
        framebuffers: Vec<VkFramebuffer>,
        img_count: u32,
        depth_img: VkImage,
        depth_img_view: VkImageView,
}

impl VkSwapchain {
        fn new(
                window: &Window,
                instance: &ash::Instance,
                surface: &Arc<VkSurface>,
                physical_device: vk::PhysicalDevice,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                old_swapchain: vk::SwapchainKHR,
        ) -> Result<Self, Box<dyn Error>> {
                let format = Self::choose_format(surface, physical_device)?;
                debug!("VkSwapchain format ({:?})", format);

                let surface_capabilities = unsafe {
                        surface.loader()
                                .get_physical_device_surface_capabilities(physical_device, ***surface)?
                };

                let desired_img_count = na::clamp(
                        3,
                        surface_capabilities.min_image_count,
                        match surface_capabilities.max_image_count {
                                0 => u32::MAX,
                                _ => surface_capabilities.max_image_count,
                        },
                );

                debug!("VkSwapchain image count: {}", desired_img_count);

                let extent = match surface_capabilities.current_extent.width {
                        u32::MAX => vk::Extent2D {
                                width: window.inner_size().width,
                                height: window.inner_size().height,
                        },
                        _ => surface_capabilities.current_extent,
                };
                debug!("VkSwapchain extent: {:?}", extent);

                let pre_transform = surface_capabilities.current_transform;

                let present_mode = Self::choose_present_mode(&surface, physical_device)?;
                debug!("VkSwapchain present mode: {:?}", present_mode);

                let loader = Swapchain::new(instance, &***device);

                let swch_cinfo = vk::SwapchainCreateInfoKHR::builder()
                        .surface(***surface)
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

                let color_imgs = unsafe { loader.get_swapchain_images(handle)? };

                let img_count = color_imgs.len() as u32;

                let color_img_views = color_imgs
                        .iter()
                        .map(|&img| {
                                let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                        .image(img)
                                        .view_type(vk::ImageViewType::TYPE_2D)
                                        .format(format.format)
                                        .components(vk::ComponentMapping::default())
                                        .subresource_range(vk::ImageSubresourceRange {
                                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                                base_mip_level: 0,
                                                level_count: 1,
                                                base_array_layer: 0,
                                                layer_count: 1,
                                        });

                                unsafe { VkImageView::new(device, &img_view_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkImageView>>>()?;

                let depth_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format: DEPTH_STENCIL_FORMAT,
                                extent: Extent3D {
                                        width: extent.width,
                                        height: extent.height,
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples: vk::SampleCountFlags::TYPE_1,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::NONE,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(allocator, &depth_img_cinfo)?
                };

                let depth_img_view = unsafe {
                        let depth_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *depth_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: DEPTH_STENCIL_FORMAT,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::DEPTH,
                                        base_mip_level: 0,
                                        level_count: 1,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &depth_img_view_cinfo)?
                };

                Ok(Self {
                        loader,
                        surface: Arc::clone(surface),

                        handle,
                        format,
                        extent,
                        present_mode,
                        color_imgs,
                        color_img_views,
                        framebuffers: vec![],
                        img_count,
                        depth_img,
                        depth_img_view,
                })
        }

        pub fn create_framebuffers(&mut self, device: &Arc<VkDevice>, render_pass: vk::RenderPass) -> VkResult<()> {
                assert!(self.framebuffers.is_empty());

                self.framebuffers = self
                        .color_img_views
                        .iter()
                        .map(|color_img_view| {
                                let attachments = [**color_img_view, *self.depth_img_view];

                                let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                        .render_pass(render_pass)
                                        .attachments(&attachments)
                                        .width(self.extent.width)
                                        .height(self.extent.height)
                                        .layers(1);

                                unsafe { VkFramebuffer::new(device, &framebuffer_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkFramebuffer>>>()?;

                Ok(())
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

        fn choose_format(surface: &VkSurface, physical_device: vk::PhysicalDevice) -> VkResult<vk::SurfaceFormatKHR> {
                let formats = unsafe {
                        surface.loader()
                                .get_physical_device_surface_formats(physical_device, **surface)?
                };

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
                surface: &VkSurface,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::PresentModeKHR> {
                let modes = unsafe {
                        surface.loader()
                                .get_physical_device_surface_present_modes(physical_device, **surface)?
                };

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

impl Deref for VkSwapchain {
        type Target = vk::SwapchainKHR;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkSwapchain {
        fn drop(&mut self) {
                unsafe { self.loader.destroy_swapchain(self.handle, None) };
        }
}

pub struct VkReusableCommandBuffer {
        handle: vk::CommandBuffer,
        fence: vk::Fence,
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

                Ok(Self { handle, fence })
        }

        pub fn new_vec(device: &ash::Device, cmd_pool: vk::CommandPool, count: u32) -> VkResult<Vec<Self>> {
                let cmd_buffer_ainfo = vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .command_buffer_count(count)
                        .level(vk::CommandBufferLevel::PRIMARY);

                let handles = unsafe { device.allocate_command_buffers(&cmd_buffer_ainfo)? };

                let fence_cinfo = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

                handles.iter()
                        .map(|&handle| {
                                let fence = unsafe { device.create_fence(&fence_cinfo, None)? };

                                Ok(Self { handle, fence })
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
        ) -> Result<(), Box<dyn Error>>
        where
                F: FnOnce(&ash::Device, vk::CommandBuffer) -> Result<(), Box<dyn Error>>,
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
                        f(&device, self.handle)?;
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

impl Deref for VkReusableCommandBuffer {
        type Target = vk::CommandBuffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

struct Matrices3D {
        model: Mat4,
        view: Mat4,
        proj: Mat4,
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
