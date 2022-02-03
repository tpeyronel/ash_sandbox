use std::{
        borrow::Cow,
        error::Error,
        ffi::{c_void, CStr, CString},
        os::raw::c_char,
};

extern crate vk_mem as vma;

use std::rc::Rc;

use ash::{extensions::khr::Swapchain, prelude::VkResult, vk};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use winit::window::Window;

use super::vk_wrapper::{VkCommandPool, VkDebugUtilsMessenger, VkDescriptorPool, VkDevice, VkInstance, VkSurface};
use crate::{
        scoped_timer::{ScopedTimer, TimePrefix},
        vk::vk_wrapper::{VkPhysicalDevice, VkQueueFamilyIndices, VkQueues},
};

macro_rules! cstring {
        ($s:expr) => {
                CString::new($s).unwrap()
        };
}

pub struct VkContext {
        pub instance: Rc<VkInstance>,

        _debug_utils_messenger: Option<VkDebugUtilsMessenger>,

        pub surface: Rc<VkSurface>,

        pub pdevice: VkPhysicalDevice,

        pub device: Rc<VkDevice>,

        _qfamilyi: VkQueueFamilyIndices,
        pub queues: VkQueues,

        pub allocator: Rc<vma::Allocator>,
        pub cmd_pool: Rc<VkCommandPool>,
        pub dst_pool: VkDescriptorPool,
}

#[cfg(all(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = true;
#[cfg(not(debug_assertions))]
const ENABLE_VALIDATION_LAYERS: bool = false;

impl VkContext {
        pub fn new(window: Rc<Window>) -> Result<Self, Box<dyn Error>> {
                let _t = ScopedTimer::new("Initialized VkContext in: ", TimePrefix::Base);

                let entry = Rc::new(unsafe { ash::Entry::load()? });

                let instance = Self::create_instance(&window, &entry)?;
                trace!("Created VkInstance");

                let debug_utils_messenger = if !ENABLE_VALIDATION_LAYERS {
                        None
                } else {
                        let debug_messenger = {
                                let debug_cinfo = Self::create_debug_utils_messenger_cinfo();

                                unsafe { VkDebugUtilsMessenger::new(&entry, &instance, &debug_cinfo)? }
                        };
                        trace!("Created VkDebugUtilsMessenger");

                        Some(debug_messenger)
                };

                let surface = Rc::new(unsafe {
                        VkSurface::new(Rc::clone(&window), Rc::clone(&entry), Rc::clone(&instance))?
                });
                trace!("Created VkSurface");

                let (pdevice, qfamilyi) = VkPhysicalDevice::new(&instance, &surface)?;

                trace!("Chose VkPhysicalDevice");
                info!("Chosen physical device: {:?}", unsafe {
                        CStr::from_ptr(pdevice.props.device_name.as_ptr())
                });
                info!("Queue family indices: {:?}", &qfamilyi);

                let (device, queues) = Self::create_device(&instance, *pdevice, &qfamilyi)?;
                trace!("Created VkDevice");

                let allocator = Self::create_allocator(&instance, *pdevice, &device)?;
                trace!("Created VmaAllocator");

                let cmd_pool = Self::create_command_pool(&device, &qfamilyi)?;
                trace!("Created VkCommandPool");

                let dst_pool = Self::create_descriptor_pool(&device)?;
                trace!("Created VkDescriptorPool");

                Ok(Self {
                        instance,

                        _debug_utils_messenger: debug_utils_messenger,

                        surface,

                        pdevice,

                        device,

                        _qfamilyi: qfamilyi,
                        queues,

                        allocator,

                        cmd_pool,

                        dst_pool,
                })
        }

        fn create_instance(window: &Window, entry: &Rc<ash::Entry>) -> Result<Rc<VkInstance>, Box<dyn Error>> {
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
                                .application_version(vk::make_api_version(0, 1, 0, 0))
                                .engine_name(&app_name)
                                .engine_version(vk::make_api_version(0, 1, 0, 0))
                                .api_version(vk::make_api_version(0, 1, 2, 0));

                        let mut instance_cinfo = vk::InstanceCreateInfo::builder()
                                .application_info(&app_info)
                                .enabled_layer_names(&req_layers_raw)
                                .enabled_extension_names(&req_extensions_raw);

                        let debug_info = Self::create_debug_utils_messenger_cinfo();

                        if ENABLE_VALIDATION_LAYERS {
                                instance_cinfo.p_next =
                                        &debug_info as *const vk::DebugUtilsMessengerCreateInfoEXT as *const c_void;
                        }

                        Ok(Rc::new(VkInstance::new(entry, &instance_cinfo)?))
                }
        }

        fn create_debug_utils_messenger_cinfo() -> vk::DebugUtilsMessengerCreateInfoEXT {
                vk::DebugUtilsMessengerCreateInfoEXT::builder()
                        .message_severity(
                                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                        )
                        .message_type(
                                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                        )
                        .pfn_user_callback(Some(vk_debug_callback))
                        .build()
        }

        fn create_device(
                instance: &Rc<VkInstance>,
                physical_device: vk::PhysicalDevice,
                q_family_i: &VkQueueFamilyIndices,
        ) -> Result<(Rc<VkDevice>, VkQueues), Box<dyn Error>> {
                let memory_budget_ext = CStr::from_bytes_with_nul(b"VK_EXT_memory_budget\0").unwrap();
                let shader_non_semantic_info_ext = CStr::from_bytes_with_nul(b"VK_KHR_shader_non_semantic_info\0").unwrap();

                let req_device_extensions_raw = vec![Swapchain::name().as_ptr(), memory_budget_ext.as_ptr(), shader_non_semantic_info_ext.as_ptr()];
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

                let device = unsafe { Rc::new(VkDevice::new(instance, physical_device, &device_cinfo)?) };

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
        ) -> vma::Result<Rc<vma::Allocator>> {
                let allocator_cinfo = vma::AllocatorCreateInfo {
                        physical_device,
                        device: device.clone(),
                        instance: instance.clone(),
                        flags: vma::AllocatorCreateFlags::NONE,
                        preferred_large_heap_block_size: 0,
                        frame_in_use_count: 0,
                        heap_size_limits: None,
                };

                Ok(Rc::new(vma::Allocator::new(&allocator_cinfo)?))
        }

        fn create_descriptor_pool(device: &Rc<VkDevice>) -> VkResult<VkDescriptorPool> {
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

                let dst_pool_cinfo = vk::DescriptorPoolCreateInfo::builder()
                        .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                        .pool_sizes(&pool_sizes)
                        .max_sets(1000);

                unsafe { VkDescriptorPool::new(device, &dst_pool_cinfo) }
        }

        fn create_command_pool(
                device: &Rc<VkDevice>,
                q_family_i: &VkQueueFamilyIndices,
        ) -> VkResult<Rc<VkCommandPool>> {
                let cmd_pool_cinfo = vk::CommandPoolCreateInfo::builder()
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                        .queue_family_index(q_family_i.graphics);

                Ok(Rc::new(unsafe { VkCommandPool::new(device, &cmd_pool_cinfo)? }))
        }
}

/*impl Renderer for VkContext {
        fn draw(&mut self, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>> {
                let window_size = self.window.inner_size();
                if window_size.width == 0 || window_size.height == 0 {
                        return Ok(());
                }

                if self.swapchain_outdated_causes != VkSwapchainOutdatedCauses::NONE {
                        self.recreate_swapchain()?;
                }

                let frame_img_avail_semaphore = &self.img_avail_semaphores[self.frame_i];

                let (img_i, suboptimal) = unsafe {
                        self.swapchain
                                .acquire_next_image(u64::MAX, **frame_img_avail_semaphore, vk::Fence::null())?
                };

                if suboptimal {
                        self.swapchain_outdated_causes = VkSwapchainOutdatedCauses::SUBOPTIMAL;
                }

                let frame_draw_cmd_buffer = &self.draw_cmd_buffers[self.frame_i];
                let frame_present_complete_semaphore = &self.present_complete_semaphores[self.frame_i];

                let frame_framebuffer = &self.swapchain.framebuffers[img_i as usize];

                let time = self.creation_instant.elapsed().as_secs_f32();
                let intensity = ((time.sin() + 1.0) / 2.0) * 0.05;

                let clear_values = [
                        vk::ClearValue {
                                color: vk::ClearColorValue {
                                        float32: [intensity, intensity, intensity, 1.0],
                                },
                        },
                        vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue {
                                        depth:   1.0,
                                        stencil: 0,
                                },
                        },
                ];

                self.update_matrices_buffer(&self.matrices_buffers[img_i as usize])?;

                let mut imgui_renderer = self.imgui_renderer.take().unwrap();

                frame_draw_cmd_buffer.record_and_submit(
                        &self.device,
                        self.queues.graphics,
                        &[**frame_img_avail_semaphore],
                        &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
                        &[**frame_present_complete_semaphore],
                        |device, draw_cmd_buffer| unsafe {
                                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                        .render_pass(*self.render_pass)
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
                                        *self.graphics_pipeline,
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
                                        *self.graphics_pipeline_layout,
                                        0,
                                        &[self.dst_sets[img_i as usize]],
                                        &[],
                                );

                                for _ in 0..1 {
                                        device.cmd_draw_indexed(draw_cmd_buffer, 36, 1, 0, 0, 0);
                                }

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
                                        .wait_semaphores(&[**frame_present_complete_semaphore])
                                        .swapchains(&[*self.swapchain])
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
                                Err(err) => return Err(err.into()),
                                _ => {},
                        };
                }

                self.frame_i = (self.frame_i + 1) % (self.swapchain.img_count as usize);
                self.frame_counter += 1;

                Ok(())
        }
}*/

impl Drop for VkContext {
        fn drop(&mut self) {
                unsafe {
                        trace!("Destroying VkContext...");

                        if let Err(err) = self.device.device_wait_idle() {
                                error!("Error occurred while waiting device idle: {}", err);
                        }
                }
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
