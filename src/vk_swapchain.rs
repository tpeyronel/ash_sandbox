use std::{error::Error, ops::Deref, sync::Arc};

use ash::{extensions::khr::Swapchain, prelude::VkResult, version::InstanceV1_0, vk};
use bitflags::bitflags;
use log::{debug, trace};

use crate::{
        vk_image::{VkImage, VkImageCreateInfo},
        vk_wrapper::{VkDevice, VkFramebuffer, VkImageView, VkInstance, VkSurface},
        vkma_error::VkmaResult,
};

pub struct VkSwapchain {
        loader: Swapchain,

        window:          Arc<winit::window::Window>,
        instance:        Arc<VkInstance>,
        surface:         Arc<VkSurface>,
        physical_device: vk::PhysicalDevice,
        device:          Arc<VkDevice>,
        allocator:       Arc<vma::Allocator>,

        handle: vk::SwapchainKHR,

        pub color_format: vk::SurfaceFormatKHR,
        pub depth_format: vk::Format,
        pub extent:       vk::Extent2D,
        pub present_mode: vk::PresentModeKHR,
        pub samples:      vk::SampleCountFlags,

        pub color_img:      VkImage,
        pub color_img_view: VkImageView,

        pub depth_img:      VkImage,
        pub depth_img_view: VkImageView,

        pub resolve_imgs:      Vec<vk::Image>,
        pub resolve_img_views: Vec<VkImageView>,
        pub img_count:         u32,
        pub framebuffers:      Vec<VkFramebuffer>,
}

impl VkSwapchain {
        pub fn new(
                window: &Arc<winit::window::Window>,
                instance: &Arc<VkInstance>,
                surface: &Arc<VkSurface>,
                physical_device: vk::PhysicalDevice,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
        ) -> Result<Self, Box<dyn Error>> {
                let color_format = Self::choose_color_format(surface, physical_device)?;
                debug!("VkSwapchain color format ({:?})", color_format);
                let depth_format = vk::Format::D24_UNORM_S8_UINT;

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

                let loader = Swapchain::new(&***instance, &***device);

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
                        .old_swapchain(vk::SwapchainKHR::null());

                let handle = unsafe { loader.create_swapchain(&swch_cinfo, None)? };

                let samples = Self::choose_sample_count(instance, physical_device);
                debug!("Swapchain samples: {:?}", samples);

                let (color_img, color_img_view) =
                        Self::create_color_img_resources(device, allocator, color_format.format, &extent, samples)?;
                let (depth_img, depth_img_view) =
                        Self::create_depth_img_resources(device, allocator, depth_format, &extent, samples)?;

                let resolve_imgs = unsafe { loader.get_swapchain_images(handle)? };
                let resolve_img_views = Self::create_resolve_img_views(device, &resolve_imgs, color_format.format)?;

                let img_count = resolve_imgs.len() as u32;

                Ok(Self {
                        loader,

                        window: Arc::clone(window),
                        instance: Arc::clone(instance),
                        surface: Arc::clone(surface),
                        physical_device,
                        device: Arc::clone(device),
                        allocator: Arc::clone(allocator),

                        handle,

                        color_format,
                        depth_format,

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

        pub fn recreate(&mut self) -> VkmaResult<VkSwapchainRecreationInfo> {
                self.framebuffers.clear();

                let mut recreation_info = VkSwapchainRecreationInfo {
                        color_format_changed: false,
                        extent_changed:       false,
                        samples_changed:      false,
                        img_count_changed:    false,
                };

                let color_format = Self::choose_color_format(&self.surface, self.physical_device)?;
                if color_format != self.color_format {
                        recreation_info.color_format_changed = true;
                }
                self.color_format = color_format;
                debug!("VkSwapchain color format ({:?})", color_format);

                let depth_format = vk::Format::D24_UNORM_S8_UINT;
                self.depth_format = depth_format;

                let surface_capabilities = unsafe {
                        self.surface
                                .loader()
                                .get_physical_device_surface_capabilities(self.physical_device, **self.surface)?
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
                                width:  self.window.inner_size().width,
                                height: self.window.inner_size().height,
                        },
                        _ => surface_capabilities.current_extent,
                };
                if extent != self.extent {
                        recreation_info.extent_changed = true;
                }
                self.extent = extent;
                debug!("VkSwapchain extent: {:?}", extent);

                let pre_transform = surface_capabilities.current_transform;

                let present_mode = Self::choose_present_mode(&self.surface, self.physical_device)?;
                debug!("VkSwapchain present mode: {:?}", present_mode);
                self.present_mode = present_mode;

                let loader = Swapchain::new(&**self.instance, &**self.device);

                let swch_cinfo = vk::SwapchainCreateInfoKHR {
                        surface: **self.surface,
                        min_image_count: desired_img_count,
                        image_format: color_format.format,
                        image_color_space: color_format.color_space,
                        image_extent: extent,
                        image_array_layers: 1,
                        image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                        image_sharing_mode: vk::SharingMode::EXCLUSIVE,
                        queue_family_index_count: 0,
                        p_queue_family_indices: std::ptr::null(),
                        pre_transform,
                        composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
                        present_mode,
                        clipped: vk::TRUE,
                        old_swapchain: self.handle,
                        ..Default::default()
                };

                let old_handle = self.handle;
                self.handle = unsafe { loader.create_swapchain(&swch_cinfo, None)? };
                unsafe { self.loader.destroy_swapchain(old_handle, None) };

                let samples = Self::choose_sample_count(&self.instance, self.physical_device);
                if samples != self.samples {
                        recreation_info.samples_changed = true;
                }
                self.samples = samples;
                debug!("Swapchain samples: {:?}", samples);

                if recreation_info.color_format_changed
                        || recreation_info.extent_changed
                        || recreation_info.samples_changed
                {
                        let (color_img, color_img_view) = Self::create_color_img_resources(
                                &self.device,
                                &self.allocator,
                                color_format.format,
                                &extent,
                                samples,
                        )?;
                        self.color_img = color_img;
                        self.color_img_view = color_img_view;

                        let (depth_img, depth_img_view) = Self::create_depth_img_resources(
                                &self.device,
                                &self.allocator,
                                depth_format,
                                &extent,
                                samples,
                        )?;

                        self.depth_img = depth_img;
                        self.depth_img_view = depth_img_view;
                }

                self.resolve_imgs = unsafe { loader.get_swapchain_images(self.handle)? };
                self.resolve_img_views =
                        Self::create_resolve_img_views(&self.device, &self.resolve_imgs, color_format.format)?;

                let img_count = self.resolve_imgs.len() as u32;
                if img_count != self.img_count {
                        recreation_info.img_count_changed = true;
                }
                self.img_count = img_count;

                Ok(recreation_info)
        }

        pub fn create_framebuffers(&mut self, render_pass: vk::RenderPass) -> VkResult<()> {
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

                                unsafe { VkFramebuffer::new(&self.device, &framebuffer_cinfo) }
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

        fn create_color_img_resources(
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                format: vk::Format,
                extent: &vk::Extent2D,
                samples: vk::SampleCountFlags,
        ) -> VkmaResult<(VkImage, VkImageView)> {
                let color_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format,
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
                                format,
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

                Ok((color_img, color_img_view))
        }

        fn create_depth_img_resources(
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                format: vk::Format,
                extent: &vk::Extent2D,
                samples: vk::SampleCountFlags,
        ) -> VkmaResult<(VkImage, VkImageView)> {
                let depth_img = unsafe {
                        let depth_img_cinfo = VkImageCreateInfo {
                                image_type: vk::ImageType::TYPE_2D,
                                format,
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

                Ok((depth_img, depth_img_view))
        }

        fn create_resolve_img_views(
                device: &Arc<VkDevice>,
                resolve_imgs: &Vec<vk::Image>,
                format: vk::Format,
        ) -> VkResult<Vec<VkImageView>> {
                resolve_imgs
                        .iter()
                        .map(|&image| {
                                let img_view_cinfo = vk::ImageViewCreateInfo {
                                        image,
                                        view_type: vk::ImageViewType::TYPE_2D,
                                        format,
                                        components: vk::ComponentMapping::default(),
                                        subresource_range: vk::ImageSubresourceRange {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                base_mip_level:   0,
                                                level_count:      1,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        },
                                        ..Default::default()
                                };

                                unsafe { VkImageView::new(device, &img_view_cinfo) }
                        })
                        .collect::<VkResult<Vec<VkImageView>>>()
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

bitflags! {
        pub struct VkSwapchainOutdatedCauses: u32 {
                const NONE = 0b00000000;
                const WINDOW_RESIZE = 0b00000001;
                const SUBOPTIMAL = 0b00000010;
                const OUT_OF_DATE = 0b00000100;
        }
}

pub struct VkSwapchainRecreationInfo {
        pub color_format_changed: bool,
        pub extent_changed:       bool,
        pub samples_changed:      bool,
        pub img_count_changed:    bool,
}
