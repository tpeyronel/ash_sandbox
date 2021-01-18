use std::{error::Error, ops::Deref, sync::Arc};

use ash::{extensions::khr::Swapchain, prelude::VkResult, version::InstanceV1_0, vk};
use log::{debug, trace};

use crate::{
        vk_image::{VkImage, VkImageCreateInfo},
        vk_wrapper::{VkDevice, VkFramebuffer, VkImageView, VkSurface},
};

pub struct VkSwapchain {
        loader:  Swapchain,
        surface: Arc<VkSurface>,

        handle:                vk::SwapchainKHR,
        pub color_format:      vk::SurfaceFormatKHR,
        pub depth_format:      vk::Format,
        pub extent:            vk::Extent2D,
        pub present_mode:      vk::PresentModeKHR,
        pub samples:           vk::SampleCountFlags,
        pub color_img:         VkImage,
        pub color_img_view:    VkImageView,
        pub depth_img:         VkImage,
        pub depth_img_view:    VkImageView,
        pub resolve_imgs:      Vec<vk::Image>,
        pub resolve_img_views: Vec<VkImageView>,
        pub img_count:         u32,
        pub framebuffers:      Vec<VkFramebuffer>,
}

impl VkSwapchain {
        pub fn new(
                window: &winit::window::Window,
                instance: &ash::Instance,
                surface: &Arc<VkSurface>,
                physical_device: vk::PhysicalDevice,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                old_swapchain: vk::SwapchainKHR,
        ) -> Result<Self, Box<dyn Error>> {
                let color_format = Self::choose_color_format(surface, physical_device)?;
                debug!("VkSwapchain color format ({:?})", color_format);

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
                                width:  window.inner_size().width,
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
                        .image_color_space(color_format.color_space)
                        .image_format(color_format.format)
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

                let samples = Self::choose_sample_count(instance, physical_device);
                debug!("Swapchain samples: {:?}", samples);

                let color_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format: color_format.format,
                                extent: vk::Extent3D {
                                        width:  extent.width,
                                        height: extent.height,
                                        depth:  1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::TRANSIENT_ATTACHMENT
                                        | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::NONE,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(allocator, &depth_img_cinfo)?
                };

                let color_img_view = unsafe {
                        let color_img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *color_img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: color_format.format,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::COLOR,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &color_img_view_cinfo)?
                };

                let depth_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format: vk::Format::D24_UNORM_S8_UINT,
                                extent: vk::Extent3D {
                                        width:  extent.width,
                                        height: extent.height,
                                        depth:  1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples,
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
                                format: vk::Format::D24_UNORM_S8_UINT,
                                components: vk::ComponentMapping::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask:      vk::ImageAspectFlags::DEPTH,
                                        base_mip_level:   0,
                                        level_count:      1,
                                        base_array_layer: 0,
                                        layer_count:      1,
                                },
                                ..vk::ImageViewCreateInfo::default()
                        };

                        VkImageView::new(device, &depth_img_view_cinfo)?
                };

                let resolve_imgs = unsafe { loader.get_swapchain_images(handle)? };

                let resolve_img_views = resolve_imgs
                        .iter()
                        .map(|&img| {
                                let img_view_cinfo = vk::ImageViewCreateInfo::builder()
                                        .image(img)
                                        .view_type(vk::ImageViewType::TYPE_2D)
                                        .format(color_format.format)
                                        .components(vk::ComponentMapping::default())
                                        .subresource_range(vk::ImageSubresourceRange {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                base_mip_level:   0,
                                                level_count:      1,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        });

                                unsafe { VkImageView::new(device, &img_view_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkImageView>>>()?;
                let img_count = resolve_imgs.len() as u32;

                Ok(Self {
                        loader,
                        surface: Arc::clone(surface),

                        handle,

                        color_format,
                        depth_format: vk::Format::D24_UNORM_S8_UINT,

                        extent,
                        present_mode,
                        samples,

                        color_img,
                        color_img_view,

                        depth_img,
                        depth_img_view,

                        resolve_imgs,
                        resolve_img_views,
                        img_count,

                        framebuffers: vec![],
                })
        }

        pub fn create_framebuffers(&mut self, device: &Arc<VkDevice>, render_pass: vk::RenderPass) -> VkResult<()> {
                assert!(self.framebuffers.is_empty());

                self.framebuffers = self
                        .resolve_img_views
                        .iter()
                        .map(|resolve_img_view| {
                                let attachments = [*self.color_img_view, *self.depth_img_view, **resolve_img_view];

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

        pub unsafe fn acquire_next_image(
                &self,
                timeout: u64,
                semaphore: vk::Semaphore,
                fence: vk::Fence,
        ) -> VkResult<(u32, bool)> {
                self.loader.acquire_next_image(self.handle, timeout, semaphore, fence)
        }

        pub unsafe fn queue_present(&self, queue: vk::Queue, present_info: &vk::PresentInfoKHR) -> VkResult<bool> {
                self.loader.queue_present(queue, present_info)
        }

        fn choose_color_format(
                surface: &VkSurface,
                physical_device: vk::PhysicalDevice,
        ) -> VkResult<vk::SurfaceFormatKHR> {
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

        fn choose_sample_count(instance: &ash::Instance, physical_device: vk::PhysicalDevice) -> vk::SampleCountFlags {
                let limits = unsafe { instance.get_physical_device_properties(physical_device).limits };

                let avail_samples = limits.framebuffer_color_sample_counts & limits.framebuffer_depth_sample_counts;

                if avail_samples.contains(vk::SampleCountFlags::TYPE_64) {
                        vk::SampleCountFlags::TYPE_64
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_32) {
                        vk::SampleCountFlags::TYPE_32
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_16) {
                        vk::SampleCountFlags::TYPE_16
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_8) {
                        vk::SampleCountFlags::TYPE_8
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_4) {
                        vk::SampleCountFlags::TYPE_4
                } else if avail_samples.contains(vk::SampleCountFlags::TYPE_2) {
                        vk::SampleCountFlags::TYPE_2
                } else {
                        vk::SampleCountFlags::TYPE_1
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
