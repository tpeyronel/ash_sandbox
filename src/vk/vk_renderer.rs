use ash::{prelude::VkResult, vk};
use glam::Vec4Swizzles;
use slotmap::SecondaryMap;
use std::{rc::Rc, slice, time::Instant};

use bevy_ecs::prelude::World;
use crossbeam_channel::Receiver;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use shader_resource_derive::ShaderStruct;
use winit::{dpi::PhysicalSize, window::Window};

use super::{
        vk_asset_manager::{VkAssetManager, VkDescriptorSetIndex},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_image::{TransitionImageLayoutInfo, VkImage, VkImageCreateInfo},
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauseFlags},
        vk_util,
        vk_wrapper::{VkImageView, VkSampler, VkSemaphore},
};
use crate::{
        application::{InterpGlobalTransform, ShaderSettings},
        asset_manager::{
                AssetManager, AssetManagerEvent, CubemapId, MaterialId, MaterialMesh, MeshId, ShaderId,
                ShaderResourceData,
        },
        components::{ActiveCamera, DirectionalLight, PointLight, ProjectionCamera, Spotlight},
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE, SHADOW_MAP_HEIGHT, SHADOW_MAP_WIDTH},
        model_instance_manager::ModelInstance,
        my_glm::*,
        renderer::{Cluster, Renderer},
        shader_resources::{
                SHADER_RESOURCE_BILLBOARD_DATA, SHADER_RESOURCE_BRDF_LUT, SHADER_RESOURCE_CUBE_SHADOW_MAP,
                SHADER_RESOURCE_FRUSTUM_CLUSTERS, SHADER_RESOURCE_INPUT_FRAMEBUFFER, SHADER_RESOURCE_IRRADIANCE_MAP,
                SHADER_RESOURCE_MATERIAL_DATA, SHADER_RESOURCE_OBJECT_MATRICES, SHADER_RESOURCE_PREFILTERED_MAP,
                SHADER_RESOURCE_SHADER_SETTINGS, SHADER_RESOURCE_SHADOW_MAP, SHADER_RESOURCE_SKYBOX,
                SHADER_RESOURCE_WORLD_LIGHTS, SHADER_RESOURCE_WORLD_MATRICES, SHADER_RESOURCE_WORLD_POINT_LIGHTS,
        },
        skybox::Skybox,
        util::{RefIntoBytesSlice, RefIntoSlice},
        vk::{
                vk_asset_manager::VkGraphicsShader, vk_image_subresource_range::ImageSubresourceRangeUtil,
                vk_wrapper::HasVkHandle,
        },
};
use crate::{
        constants::{DESIRED_SWAPCHAIN_IMG_COUNT, MAX_CONCURRENT_FRAMES},
        AnyResult,
};

use bytemuck::NoUninit;

pub struct VkRenderer {
        window: Rc<Window>,
        asset_manager_event_rx: Receiver<AssetManagerEvent>,

        context: VkContext,
        vk_asset_manager: VkAssetManager,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags,

        resolve_sampler: VkSampler,

        shadow_map_depth_img: VkImage,
        shadow_map_depth_img_view: VkImageView,
        shadow_map_sampler: VkSampler,

        cube_shadow_map_img: VkImage,
        cube_shadow_map_img_view: VkImageView, // For entire cube
        cube_shadow_map_img_views: [VkImageView; 6],
        cube_shadow_map_depth_img: VkImage,
        cube_shadow_map_depth_img_view: VkImageView,
        cube_shadow_map_sampler: VkSampler,

        setup_cmd_buffer: VkReusableCommandBuffer,

        max_concurrent_frames: usize,
        frames_data: Vec<VkFrameData>,

        imgui_renderer: Option<imgui_rs_vulkan_renderer::Renderer>,

        creation_instant: Instant,
        framei: usize,
}

impl VkRenderer {
        pub fn new(
                window: Rc<Window>,
                imguic: &mut imgui::Context,
                asset_manager_event_rx: Receiver<AssetManagerEvent>,
        ) -> AnyResult<Self> {
                let mut context = VkContext::new(Rc::clone(&window))?;

                let swapchain = VkSwapchain::new(Rc::clone(&window), &context, DESIRED_SWAPCHAIN_IMG_COUNT)?;
                trace!("Created VkSwapchain");

                let resolve_sampler = Self::create_resolve_sampler(&context)?;

                let shadow_map_depth_format = swapchain.depth_format;

                let cube_shadow_map_color_format = Self::choose_cube_shadow_map_color_format(&context)?;

                let cube_shadow_map_depth_format = shadow_map_depth_format;

                let (shadow_map_depth_img, shadow_map_depth_img_view) =
                        Self::create_shadow_map_depth_img_and_view(&context, shadow_map_depth_format)?;

                let shadow_map_sampler = Self::create_shadow_map_sampler(&context)?;

                let (cube_shadow_map_img, cube_shadow_map_img_view, cube_shadow_map_img_views) =
                        Self::create_cube_shadow_map_img_and_views(&context, cube_shadow_map_color_format)?;

                let (cube_shadow_map_depth_img, cube_shadow_map_depth_img_view) =
                        Self::create_cube_shadow_map_depth_img_and_view(&context, cube_shadow_map_depth_format)?;

                let cube_shadow_map_sampler = Self::create_cube_shadow_map_sampler(&context)?;

                let setup_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&context.device), Rc::clone(&context.cmd_pool))?;
                trace!("Allocated VkCommandBuffers");

                let max_concurrent_frames = MAX_CONCURRENT_FRAMES;
                let frames_data = (0..max_concurrent_frames)
                        .map(|_| VkFrameData::new(&mut context))
                        .collect::<AnyResult<Vec<VkFrameData>>>()?;

                let vk_asset_manager = VkAssetManager::new(&mut context, swapchain.samples, max_concurrent_frames)?;
                trace!("Created VkAssetManager");

                let imgui_renderer_options = imgui_rs_vulkan_renderer::Options {
                        in_flight_frames: max_concurrent_frames,
                        enable_depth_test: false,
                        enable_depth_write: false,
                        subpass: 0,
                        sample_count: vk::SampleCountFlags::TYPE_1,
                };

                let imgui_renderer = Some(imgui_rs_vulkan_renderer::Renderer::with_default_allocator(
                        &**context.instance,
                        **context.pdevice,
                        (**context.device).clone(),
                        context.queues.graphics,
                        **context.cmd_pool,
                        imgui_rs_vulkan_renderer::DynamicRendering {
                                color_attachment_format: swapchain.resolve_imgs[0].format,
                                depth_attachment_format: None,
                        },
                        imguic,
                        Some(imgui_renderer_options),
                )?);

                Ok(Self {
                        window,
                        asset_manager_event_rx,

                        context,
                        vk_asset_manager,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags::NONE,

                        resolve_sampler,

                        shadow_map_depth_img,
                        shadow_map_depth_img_view,
                        shadow_map_sampler,

                        cube_shadow_map_img,
                        cube_shadow_map_img_view,
                        cube_shadow_map_img_views,
                        cube_shadow_map_depth_img,
                        cube_shadow_map_depth_img_view,
                        cube_shadow_map_sampler,

                        setup_cmd_buffer,
                        max_concurrent_frames,
                        frames_data,

                        imgui_renderer,

                        creation_instant: Instant::now(),

                        framei: 0,
                })
        }
}

impl Renderer for VkRenderer {
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()> {
                self.vk_asset_manager.process_asset_manager_events(
                        &self.context,
                        world.get_resource::<AssetManager>().unwrap(),
                        &self.asset_manager_event_rx,
                )?;

                if !self.should_render() {
                        return Ok(());
                }

                self.vk_asset_manager.notify_new_frame(self.framei)?;

                let imagei = match unsafe { self.begin_frame()? } {
                        BeginFrameResult::Draw { imagei } => imagei,
                        BeginFrameResult::Skip => return Ok(()),
                };

                let asset_manager = world.remove_resource::<AssetManager>().unwrap();

                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_SHADER_SETTINGS,
                        &ShaderResourceData::from_shader_struct(world.get_resource::<ShaderSettings>().unwrap()),
                        self.framei,
                )?;

                let cluster_grid_size = Vec3u::new(16, 9, 20);
                let cluster_grid_size = Vec4u::from((
                        cluster_grid_size,
                        cluster_grid_size.x * cluster_grid_size.y * cluster_grid_size.z,
                ));

                {
                        let clustering_workgroup_size = Vec3u::new(4, 4, 4);

                        let clusters = vec![
                                Cluster {
                                        min_point: Vec4::ZERO,
                                        max_point: Vec4::ZERO
                                };
                                cluster_grid_size.w as usize
                        ];

                        self.vk_asset_manager.provide_shader_resource(
                                &self.context,
                                &asset_manager,
                                &SHADER_RESOURCE_FRUSTUM_CLUSTERS,
                                &ShaderResourceData::from_shader_struct_field_array(&clusters),
                                self.framei,
                        )?;

                        let cmd_buffer = self.frames_data[self.framei].draw_cmd_buffer.handle();

                        let clustering_shader_id = asset_manager.shader_names()["clustering-shader"];
                        let clustering_shader = self.vk_asset_manager.compute_shader(clustering_shader_id);

                        let clustering_pipeline = self.vk_asset_manager.get_pipeline_for_compute_shader(
                                &self.context,
                                clustering_shader_id,
                                clustering_workgroup_size,
                        )?;

                        let group_count =
                                (cluster_grid_size.xyz().as_vec3() / clustering_workgroup_size.as_vec3()).ceil();

                        self.vk_asset_manager.update_world_dst_set_for_compute(
                                &self.context,
                                clustering_shader,
                                self.framei,
                        );

                        unsafe {
                                self.context.device.cmd_bind_pipeline(
                                        cmd_buffer,
                                        vk::PipelineBindPoint::COMPUTE,
                                        clustering_pipeline,
                                );

                                self.context.device.cmd_bind_descriptor_sets(
                                        cmd_buffer,
                                        vk::PipelineBindPoint::COMPUTE,
                                        *clustering_shader.compute_pipeline_layout,
                                        VkDescriptorSetIndex::World.value(),
                                        &[clustering_shader.world_dst_set[self.framei]],
                                        &[],
                                );

                                self.context.device.cmd_dispatch(
                                        cmd_buffer,
                                        group_count.x as u32,
                                        group_count.y as u32,
                                        group_count.z as u32,
                                );
                        }
                }

                let PhysicalSize { width, height } = self.window.inner_size();
                let aspect_ratio = width as f32 / height as f32;

                let camera = world.get_resource::<ActiveCamera>().unwrap().0;
                let camera_orien = world.get::<InterpGlobalTransform>(camera).unwrap().0.rotation;
                let camera_pos = world.get::<InterpGlobalTransform>(camera).unwrap().0.translation;
                let camera_projection = world.get::<ProjectionCamera>(camera).unwrap();

                let inverted_view_mat = Mat4::from_translation(camera_pos) * Mat4::from_quat(camera_orien);
                let view_mat = inverted_view_mat.inverse();

                let proj_mat = camera_projection.calc_proj_matrix(aspect_ratio);
                // let proj_mat = Mat4::orthographic_rh(-10.0, 10.0, -10.0, 10.0, 0.0, 20.0);

                let world_matrices = WorldMatrices {
                        view_pos: Vec4::from((camera_pos, 1.0)),
                        view: view_mat,
                        proj: proj_mat,
                        vp: proj_mat * view_mat,
                        inv_proj: proj_mat.inverse(),
                        cluster_grid_size,
                        near_far_viewport_size: Vec4::new(camera_projection.near(), camera_projection.far(), width as f32, height as f32),
                };

                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_WORLD_MATRICES,
                        &ShaderResourceData::from_shader_struct(&world_matrices),
                        self.framei,
                )?;

                let point_lights: Vec<_> = world
                        .query::<(&InterpGlobalTransform, &PointLight)>()
                        .iter(world)
                        .map(|(transform, light)| WorldPointLightNoShadow {
                                pos: Vec4::from((transform.0.translation, 1.0)),
                                color: Vec4::from((light.color, 1.0)),
                                kc_kl_kq: Vec4::new(light.kc, light.kl, light.kq, 0.0),
                        })
                        .collect();

                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_WORLD_POINT_LIGHTS,
                        &ShaderResourceData::from_shader_struct_field_array(&point_lights),
                        self.framei,
                )?;

                let (light_transform, point_light) = world
                        .query::<(&InterpGlobalTransform, &PointLight)>()
                        .iter(world)
                        .next()
                        .unwrap();

                let point_light_pos = light_transform.0.translation;
                let point_light_proj = Mat4::perspective_rh(
                        90.0f32.to_radians(),
                        SHADOW_MAP_WIDTH as f32 / SHADOW_MAP_HEIGHT as f32,
                        0.1,
                        20.0,
                );

                let mk_light_vp_mat = |dir: Vec3, up: Vec3| {
                        point_light_proj * Mat4::look_at_rh(point_light_pos, point_light_pos + dir, up)
                };

                let point_light_vp_mats = [
                        mk_light_vp_mat(Vec3::RIGHT, Vec3::DOWN),
                        mk_light_vp_mat(Vec3::LEFT, Vec3::DOWN),
                        mk_light_vp_mat(Vec3::UP, Vec3::BACKWARD),
                        mk_light_vp_mat(Vec3::DOWN, Vec3::FORWARD),
                        mk_light_vp_mat(Vec3::BACKWARD, Vec3::DOWN),
                        mk_light_vp_mat(Vec3::FORWARD, Vec3::DOWN),
                ];

                let point_light = WorldPointLight {
                        vp_mats: point_light_vp_mats,
                        pos: Vec4::from((point_light_pos, 1.0)),
                        color: Vec4::from((point_light.color, 1.0)),
                        kc_kl_kq: Vec4::new(point_light.kc, point_light.kl, point_light.kq, 0.0),
                };

                let dir_light_component = world.query::<&DirectionalLight>().iter(world).next().unwrap();

                let sun_dir = dir_light_component.direction.normalize_or_zero();
                let sun_pos = camera_pos - sun_dir * 100.0;

                let sun_orien = Quat::from_rotation_arc(Vec3::FORWARD, sun_dir);
                let inverted_sun_view = Mat4::from_rotation_translation(sun_orien, sun_pos);
                let sun_view = inverted_sun_view.inverse();

                let sun_proj = Mat4::orthographic_rh(-10.0, 10.0, -10.0, 10.0, 0.0, 200.0);
                let sun_vp = sun_proj * sun_view;

                let dir_light = WorldDirectionalLight {
                        vp: sun_vp,
                        direction: Vec4::from((dir_light_component.direction, 0.0)),
                        color_and_intensity: Vec4::from((dir_light_component.color, dir_light_component.intensity)),
                };

                let (spotlight_transform, spotlight_component) = world
                        .query::<(&InterpGlobalTransform, &Spotlight)>()
                        .iter(world)
                        .next()
                        .unwrap();

                let spotlight_dir = spotlight_transform.0.rotation * Vec3::new(0.0, 0.0, -1.0);
                let spotlight = WorldSpotlight {
                        pos: Vec4::from((spotlight_transform.0.translation, 1.0)),
                        dir: Vec4::from((spotlight_dir, spotlight_component.radius_angle.cos())),
                        color: Vec4::from((spotlight_component.color, 1.0)),
                        kc_kl_kq_inner: Vec4::new(
                                spotlight_component.kc,
                                spotlight_component.kl,
                                spotlight_component.kq,
                                spotlight_component.inner_radius_percentage,
                        ),
                };

                let world_lights = WorldLights {
                        dir_light,
                        point_light,
                        spotlight,
                };

                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_WORLD_LIGHTS,
                        &ShaderResourceData::from_shader_struct(&world_lights),
                        self.framei,
                )?;

                let camera_right = Vec4::from((camera_orien * Vec3::RIGHT, 0.0));
                let camera_up = Vec4::from((camera_orien * Vec3::UP, 0.0));

                let billboard_data = BillboardData {
                        billboard_center: Vec4::ZERO,
                        billboard_scale: Vec4::splat(0.5),
                        camera_right,
                        camera_up,
                };

                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_BILLBOARD_DATA,
                        &ShaderResourceData::from_shader_struct(&billboard_data),
                        self.framei,
                )?;

                /* Write BRDF LUT texture. This is done every frame, but it could be done just once. */
                self.vk_asset_manager.provide_shader_resource(
                        &self.context,
                        &asset_manager,
                        &SHADER_RESOURCE_BRDF_LUT,
                        &ShaderResourceData::Texture(asset_manager.brdf_lut),
                        self.framei,
                )?;

                let mut vk_render_scene = VkRenderScene {
                        skybox_object_matrices_offset: None,
                        mesh_instances: SecondaryMap::new(),
                };

                let mut buffer_transform_idx = 0;

                if let Some(skybox) = world.get_resource::<Skybox>() {
                        Self::update_skybox(&asset_manager, &mut self.vk_asset_manager, skybox.0, self.framei)?;

                        let object_matrices_dynamic_offset = {
                                let model = Mat4::IDENTITY;
                                let mvp = proj_mat * Mat4::from_mat3(Mat3::from_mat4(view_mat)) * model;
                                let normal = model.inverse().transpose();

                                let object_matrices = ObjectMatrices { model, mvp, normal };

                                let object_matrices_buffer = &self
                                        .vk_asset_manager
                                        .shader_resource_dynamic_buffers
                                        .get(&SHADER_RESOURCE_OBJECT_MATRICES)
                                        .unwrap()[self.framei];

                                object_matrices_buffer.write(&object_matrices, buffer_transform_idx)?
                        };

                        vk_render_scene.skybox_object_matrices_offset = Some(object_matrices_dynamic_offset);

                        buffer_transform_idx += 1;
                }

                Self::process_world_model_instances(
                        world,
                        &asset_manager,
                        &self.vk_asset_manager,
                        self.framei,
                        &world_matrices,
                        &mut buffer_transform_idx,
                        &mut vk_render_scene,
                )?;

                self.vk_asset_manager.provide_texture_shader_resource_directly(
                        &asset_manager,
                        &SHADER_RESOURCE_SHADOW_MAP,
                        self.shadow_map_depth_img_view.handle(),
                        self.shadow_map_sampler.handle(),
                        self.framei,
                )?;

                self.vk_asset_manager.provide_texture_shader_resource_directly(
                        &asset_manager,
                        &SHADER_RESOURCE_CUBE_SHADOW_MAP,
                        self.cube_shadow_map_img_view.handle(),
                        self.cube_shadow_map_sampler.handle(),
                        self.framei,
                )?;

                self.vk_asset_manager.provide_texture_shader_resource_directly(
                        &asset_manager,
                        &SHADER_RESOURCE_INPUT_FRAMEBUFFER,
                        self.swapchain.resolve_img_views[0].handle(),
                        self.resolve_sampler.handle(),
                        self.framei,
                )?;

                unsafe {
                        self.map_point_shadows(&asset_manager, &vk_render_scene)?;
                        self.map_shadows(&asset_manager, &vk_render_scene)?;

                        self.draw_scene(&asset_manager, &vk_render_scene)?;

                        self.do_postprocess(&asset_manager, imgui_draw_data)?;

                        self.end_frame(imagei)?;
                }

                world.insert_resource(asset_manager);

                Ok(())
        }

        fn on_window_resize(&mut self, _width: u32, _height: u32) {
                self.swapchain_outdated_causes
                        .insert(VkSwapchainOutdatedCauseFlags::WINDOW_RESIZE);
        }

        fn destroy(&mut self) -> AnyResult<()> {
                unsafe {
                        let _ = self.context.device.device_wait_idle();
                        drop(self.imgui_renderer.take().unwrap());
                        self.frames_data.clear();
                        self.cube_shadow_map_sampler.destroy();
                        self.cube_shadow_map_depth_img_view.destroy();
                        self.cube_shadow_map_depth_img.destroy();
                        self.cube_shadow_map_img_views.iter().for_each(|x| x.destroy());
                        self.cube_shadow_map_img_view.destroy();
                        self.cube_shadow_map_img.destroy();
                        self.setup_cmd_buffer.destroy();
                        self.shadow_map_sampler.destroy();
                        self.shadow_map_depth_img_view.destroy();
                        self.shadow_map_depth_img.destroy();
                        self.resolve_sampler.destroy();
                        self.swapchain.destroy();
                        self.vk_asset_manager.destroy(&self.context);
                        self.context.destroy();
                }

                Ok(())
        }
}

impl VkRenderer {
        fn recreate_swapchain_maybe(&mut self) -> AnyResult<()> {
                match self.swapchain_outdated_causes {
                        VkSwapchainOutdatedCauseFlags::NONE => return Ok(()),
                        // If resize is the only cause, then check that we actually need to resize
                        VkSwapchainOutdatedCauseFlags::WINDOW_RESIZE => {
                                let wsize = &self.window.inner_size();
                                let ssize = &self.swapchain.extent;

                                if (wsize.width == ssize.width) && (wsize.height == ssize.height) {
                                        self.swapchain_outdated_causes = VkSwapchainOutdatedCauseFlags::NONE;

                                        return Ok(());
                                }
                        },
                        _ => (),
                };

                trace!("Recreating VkSwapchain...");
                scoped_timer!("Recreated VkSwapchain in: ", Millis);

                unsafe { self.context.device.device_wait_idle()? };

                let _srecreation_info = self.swapchain.recreate(&self.context)?;

                self.swapchain_outdated_causes = VkSwapchainOutdatedCauseFlags::NONE;

                Ok(())
        }

        fn create_resolve_sampler(context: &VkContext) -> VkResult<VkSampler> {
                let vk_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::NEAREST,
                        min_filter: vk::Filter::NEAREST,
                        mipmap_mode: vk::SamplerMipmapMode::NEAREST,
                        address_mode_u: vk::SamplerAddressMode::REPEAT,
                        address_mode_v: vk::SamplerAddressMode::REPEAT,
                        address_mode_w: vk::SamplerAddressMode::REPEAT,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: 0,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::FLOAT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                Ok(unsafe { VkSampler::new(context, &vk_sampler_cinfo)? })
        }

        fn create_shadow_map_depth_img_and_view(
                context: &VkContext,
                depth_format: vk::Format,
        ) -> VkResult<(VkImage, VkImageView)> {
                let img = unsafe {
                        let img_cinfo = VkImageCreateInfo {
                                flags: Default::default(),
                                image_type: vk::ImageType::TYPE_2D,
                                format: depth_format,
                                extent: vk::Extent3D {
                                        width: SHADOW_MAP_WIDTH,
                                        height: SHADOW_MAP_HEIGHT,
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 1,
                                samples: vk::SampleCountFlags::TYPE_1,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::empty(),
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(context, &img_cinfo)?
                };

                let img_view = unsafe {
                        let img_view_cinfo = vk::ImageViewCreateInfo {
                                image: *img,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: depth_format,
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

                        VkImageView::new(context, &img_view_cinfo)?
                };

                Ok((img, img_view))
        }

        fn create_shadow_map_sampler(context: &VkContext) -> VkResult<VkSampler> {
                let vk_sampler_cinfo = vk::SamplerCreateInfo {
                        mag_filter: vk::Filter::NEAREST,
                        min_filter: vk::Filter::NEAREST,
                        mipmap_mode: vk::SamplerMipmapMode::LINEAR,
                        address_mode_u: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                        address_mode_v: vk::SamplerAddressMode::CLAMP_TO_BORDER,
                        address_mode_w: vk::SamplerAddressMode::REPEAT,
                        mip_lod_bias: 0.0,
                        anisotropy_enable: ENABLE_ANISOTROPY as u32,
                        max_anisotropy: 1.0,
                        compare_enable: vk::FALSE,
                        compare_op: vk::CompareOp::NEVER,
                        min_lod: 0.0,
                        max_lod: LOD_CLAMP_NONE,
                        border_color: vk::BorderColor::FLOAT_OPAQUE_WHITE,
                        unnormalized_coordinates: vk::FALSE,
                        ..Default::default()
                };

                Ok(unsafe { VkSampler::new(context, &vk_sampler_cinfo)? })
        }

        fn choose_cube_shadow_map_color_format(context: &VkContext) -> VkResult<vk::Format> {
                let candidates = [vk::Format::R32_SFLOAT, vk::Format::R16_SFLOAT];

                let features = vk::FormatFeatureFlags::COLOR_ATTACHMENT;

                vk_util::find_best_format_for_optimal_tiling(context, &candidates, features)
        }

        fn create_cube_shadow_map_img_and_views(
                context: &VkContext,
                cube_shadow_map_format: vk::Format,
        ) -> VkResult<(VkImage, VkImageView, [VkImageView; 6])> {
                let img = unsafe {
                        let img_cinfo = VkImageCreateInfo {
                                flags: vk::ImageCreateFlags::CUBE_COMPATIBLE,
                                image_type: vk::ImageType::TYPE_2D,
                                format: cube_shadow_map_format,
                                extent: vk::Extent3D {
                                        width: SHADOW_MAP_WIDTH,
                                        height: SHADOW_MAP_HEIGHT,
                                        depth: 1,
                                },
                                mip_levels: 1,
                                array_layers: 6,
                                samples: vk::SampleCountFlags::TYPE_1,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,

                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::empty(),
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(context, &img_cinfo)?
                };

                let mut img_view_cinfo = vk::ImageViewCreateInfo {
                        image: *img,
                        view_type: vk::ImageViewType::CUBE,
                        format: cube_shadow_map_format,
                        components: vk::ComponentMapping::default(),
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                base_mip_level: 0,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 6,
                        },
                        ..vk::ImageViewCreateInfo::default()
                };

                let img_view = unsafe { VkImageView::new(context, &img_view_cinfo)? };

                img_view_cinfo.view_type = vk::ImageViewType::TYPE_2D;
                img_view_cinfo.subresource_range.layer_count = 1;
                let mut mk_img_view = |l| unsafe {
                        img_view_cinfo.subresource_range.base_array_layer = l;
                        VkImageView::new(context, &img_view_cinfo)
                };

                let img_views = [
                        mk_img_view(0)?,
                        mk_img_view(1)?,
                        mk_img_view(2)?,
                        mk_img_view(3)?,
                        mk_img_view(4)?,
                        mk_img_view(5)?,
                ];

                Ok((img, img_view, img_views))
        }

        fn create_cube_shadow_map_depth_img_and_view(
                context: &VkContext,
                cube_shadow_map_depth_format: vk::Format,
        ) -> VkResult<(VkImage, VkImageView)> {
                let img = unsafe {
                        let img_cinfo = VkImageCreateInfo {
                                flags: vk::ImageCreateFlags::empty(),
                                image_type: vk::ImageType::TYPE_2D,
                                format: cube_shadow_map_depth_format,
                                extent: vk::Extent3D {
                                        width: SHADOW_MAP_WIDTH,
                                        height: SHADOW_MAP_HEIGHT,
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
                                alloc_cflags: vma::AllocationCreateFlags::empty(),
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(context, &img_cinfo)?
                };

                let img_view_cinfo = vk::ImageViewCreateInfo {
                        image: *img,
                        view_type: vk::ImageViewType::TYPE_2D,
                        format: cube_shadow_map_depth_format,
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

                let img_view = unsafe { VkImageView::new(context, &img_view_cinfo)? };

                Ok((img, img_view))
        }

        fn create_cube_shadow_map_sampler(context: &VkContext) -> VkResult<VkSampler> {
                Self::create_shadow_map_sampler(context)
        }

        fn should_render(&self) -> bool {
                if self.is_window_minimized() {
                        return false;
                }

                true
        }

        fn is_window_minimized(&self) -> bool {
                let wsize = self.window.inner_size();
                (wsize.width == 0) || (wsize.height == 0)
        }

        unsafe fn begin_frame(&mut self) -> AnyResult<BeginFrameResult> {
                self.recreate_swapchain_maybe()?;
                let frame_data = &mut self.frames_data[self.framei];

                self.context
                        .device
                        .wait_for_fences(&[*frame_data.draw_cmd_buffer.fence], true, u64::MAX)?;
                self.context.device.reset_fences(&[*frame_data.draw_cmd_buffer.fence])?;

                let imagei = {
                        let result = self.swapchain.acquire_next_image(
                                u64::MAX,
                                *frame_data.img_available_semaphore,
                                vk::Fence::null(),
                        );

                        match result {
                                Ok((imagei, suboptimal)) => {
                                        if suboptimal {
                                                self.swapchain_outdated_causes
                                                        .insert(VkSwapchainOutdatedCauseFlags::SUBOPTIMAL);
                                        }

                                        imagei
                                },
                                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                        self.swapchain_outdated_causes
                                                .insert(VkSwapchainOutdatedCauseFlags::OUT_OF_DATE);

                                        return Ok(BeginFrameResult::Skip);
                                },
                                Err(e) => return Err(e.into()),
                        }
                };

                self.context.device.reset_command_buffer(
                        *frame_data.draw_cmd_buffer,
                        vk::CommandBufferResetFlags::RELEASE_RESOURCES,
                )?;

                let cmd_buffer_binfo =
                        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                self.context
                        .device
                        .begin_command_buffer(*frame_data.draw_cmd_buffer, &cmd_buffer_binfo)?;

                // Wait for previous frame to finish before starting the new one.
                // TODO: this barrier is to prevent frame-to-frame hazards for all the different attachments.
                // Should probably find another way.
                let memory_barrier = vk::MemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                        .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                        .dst_access_mask(vk::AccessFlags2::MEMORY_WRITE);

                let dependency_info = vk::DependencyInfo::default().memory_barriers(memory_barrier.ref_into_slice());
                self.context
                        .device
                        .cmd_pipeline_barrier2(*frame_data.draw_cmd_buffer, &dependency_info);

                Ok(BeginFrameResult::Draw { imagei })
        }

        unsafe fn draw_scene(&mut self, asset_manager: &AssetManager, scene: &VkRenderScene) -> VkResult<()> {
                let frame_data = &self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;

                let time = self.creation_instant.elapsed().as_secs_f32();
                let intensity = (((time.sin() + 1.0) / 2.0) * 0.05) + 0.05;

                let color_clear_value = vk::ClearValue {
                        color: vk::ClearColorValue {
                                float32: [intensity, intensity, intensity, 1.0],
                        },
                };

                let depth_clear_value = vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                };

                {
                        let memory_barrier = vk::MemoryBarrier2::default()
                                .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                                .src_access_mask(vk::AccessFlags2::SHADER_WRITE)
                                .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                                .dst_access_mask(vk::AccessFlags2::SHADER_READ);

                        let dependency_info = vk::DependencyInfo::default().memory_barriers(memory_barrier.ref_into_slice());
                        self.context
                                .device
                                .cmd_pipeline_barrier2(*frame_data.draw_cmd_buffer, &dependency_info);
                }

                {
                        VkImage::cmd_transition_img_layout(
                                &self.context.device,
                                cmd_buffer,
                                &TransitionImageLayoutInfo {
                                        image: *self.swapchain.color_img,
                                        old_layout: vk::ImageLayout::UNDEFINED,
                                        new_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                        src_stage_mask: vk::PipelineStageFlags2::NONE,
                                        src_access_mask: vk::AccessFlags2::NONE,
                                        dst_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                        dst_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                        subresource_range: vk::ImageSubresourceRange::full_color(),
                                },
                        );

                        VkImage::cmd_transition_img_layout(
                                &self.context.device,
                                cmd_buffer,
                                &TransitionImageLayoutInfo {
                                        image: *self.swapchain.depth_img,
                                        old_layout: vk::ImageLayout::UNDEFINED,
                                        new_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                                        src_stage_mask: vk::PipelineStageFlags2::NONE,
                                        src_access_mask: vk::AccessFlags2::NONE,
                                        dst_stage_mask: vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                                                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                                        dst_access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                                                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                        subresource_range: vk::ImageSubresourceRange::full_depth(),
                                },
                        );

                        VkImage::cmd_transition_img_layout(
                                &self.context.device,
                                cmd_buffer,
                                &TransitionImageLayoutInfo {
                                        image: *self.swapchain.resolve_imgs[0],
                                        old_layout: vk::ImageLayout::UNDEFINED,
                                        new_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                        src_stage_mask: vk::PipelineStageFlags2::NONE,
                                        src_access_mask: vk::AccessFlags2::NONE,
                                        dst_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                        dst_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                        subresource_range: vk::ImageSubresourceRange::full_color(),
                                },
                        );

                        let color_attachment = vk::RenderingAttachmentInfo::default()
                                .image_view(*self.swapchain.color_img_view)
                                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                                .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                                .resolve_image_view(*self.swapchain.resolve_img_views[0])
                                .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                                .load_op(vk::AttachmentLoadOp::CLEAR)
                                .store_op(vk::AttachmentStoreOp::STORE)
                                .clear_value(color_clear_value);

                        let depth_attachment = vk::RenderingAttachmentInfo::default()
                                .image_view(*self.swapchain.depth_img_view)
                                .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                                .resolve_mode(vk::ResolveModeFlags::NONE)
                                .load_op(vk::AttachmentLoadOp::CLEAR)
                                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                                .clear_value(depth_clear_value);

                        let rendering_info = vk::RenderingInfo::default()
                                .render_area(self.swapchain.scissor)
                                .layer_count(1)
                                .color_attachments(color_attachment.ref_into_slice())
                                .depth_attachment(&depth_attachment);

                        self.context.device.cmd_begin_rendering(cmd_buffer, &rendering_info);
                }

                self.context
                        .device
                        .cmd_set_viewport(cmd_buffer, 0, slice::from_ref(&self.swapchain.viewport));
                self.context
                        .device
                        .cmd_set_scissor(cmd_buffer, 0, slice::from_ref(&self.swapchain.scissor));

                if let Some(skybox_offset) = scene.skybox_object_matrices_offset {
                        let skybox_model = asset_manager.model(asset_manager.skybox_model);
                        let skybox_material_mesh = skybox_model.meshes.first().unwrap();
                        let skybox_material = asset_manager.material(skybox_material_mesh.material);

                        let mut shader_group = SecondaryMap::new();
                        shader_group.insert(
                                skybox_material_mesh.material,
                                vec![VkMeshInstance {
                                        mesh: skybox_material_mesh.mesh,
                                        object_matrices_dynamic_offset: skybox_offset,
                                }],
                        );

                        // self.draw_shader_group(asset_manager, cmd_buffer, skybox_material.shader, &shader_group)?;
                }

                for (shader_id, shader_group) in &scene.mesh_instances {
                        self.draw_shader_group(asset_manager, cmd_buffer, shader_id, shader_group)?;
                }

                self.context.device.cmd_end_rendering(cmd_buffer);

                Ok(())
        }

        unsafe fn draw_shader_group(
                &self,
                asset_manager: &AssetManager,
                cmd_buffer: vk::CommandBuffer,
                shader_id: ShaderId,
                shader_group: &SecondaryMap<MaterialId, Vec<VkMeshInstance>>,
        ) -> VkResult<()> {
                let device = &*self.context.device;
                let framei = self.framei;

                let vk_shader = &self.vk_asset_manager.graphics_shader(shader_id);
                let vk_pipeline = self.vk_asset_manager.get_pipeline_for_graphics_shader(
                        &self.context,
                        asset_manager,
                        shader_id,
                        self.swapchain.color_format,
                        self.swapchain.depth_format,
                )?;
                device.cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, vk_pipeline);

                self.vk_asset_manager
                        .update_world_dst_set_for_graphics(&self.context, vk_shader, framei);

                device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *vk_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[vk_shader.world_dst_set[framei]],
                        &[],
                );

                for (material_id, material_group) in shader_group {
                        self.draw_material_group(asset_manager, cmd_buffer, vk_shader, material_id, material_group)?;
                }

                Ok(())
        }

        unsafe fn draw_material_group(
                &self,
                asset_manager: &AssetManager,
                cmd_buffer: vk::CommandBuffer,
                vk_shader: &VkGraphicsShader,
                material_id: MaterialId,
                material_group: &Vec<VkMeshInstance>,
        ) -> VkResult<()> {
                let device = &*self.context.device;
                let framei = self.framei;
                let material = &asset_manager.material(material_id);
                let vk_material = &self.vk_asset_manager.materials[material_id];

                // TODO: update all materials beforehand, to avoid updating the same material if its shared by multiple meshes.
                if let Some(material_buffers) = vk_material.buffers.get(&SHADER_RESOURCE_MATERIAL_DATA) {
                        material_buffers[framei].write(&MaterialData {
                                ambient_color: material.base_color_factor,
                                diffuse_color: material.base_color_factor,
                                specular_color: material.base_color_factor,
                                shininess_and_ambient_strength: Vec2::new(
                                        material.shininess,
                                        material.ambient_strength,
                                ),
                                specular_strength_and_diffuse_strength: Vec2::new(
                                        material.specular_strength,
                                        material.diffuse_strength,
                                ),
                        })?;
                }

                device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *vk_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::Material.value(),
                        &[vk_material.dst_sets[framei]],
                        &[],
                );

                for vk_mesh_instance in material_group {
                        self.draw_mesh_instance(cmd_buffer, vk_shader, vk_mesh_instance);
                }

                Ok(())
        }

        unsafe fn draw_mesh_instance(
                &self,
                cmd_buffer: vk::CommandBuffer,
                vk_shader: &VkGraphicsShader,
                vk_mesh_instance: &VkMeshInstance,
        ) {
                let device = &*self.context.device;
                let framei = self.framei;
                let vk_mesh = &self.vk_asset_manager.meshes[vk_mesh_instance.mesh];

                let mesh_dst_set = vk_shader.mesh_dst_set[framei];
                let dynamic_offset = vk_mesh_instance.object_matrices_dynamic_offset;

                device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *vk_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::Mesh.value(),
                        &[mesh_dst_set],
                        &[dynamic_offset as u32],
                );

                device.cmd_bind_vertex_buffers(
                        cmd_buffer,
                        0,
                        &[
                                *vk_mesh.positions,
                                *vk_mesh.normals,
                                *vk_mesh.tex_coords,
                                *vk_mesh.tangents,
                        ],
                        &[0, 0, 0, 0],
                );
                device.cmd_bind_index_buffer(cmd_buffer, *vk_mesh.indices.buffer, 0, vk_mesh.indices.index_type);

                device.cmd_draw_indexed(cmd_buffer, vk_mesh.indices.index_count, 1, 0, 0, 0);
        }

        unsafe fn do_postprocess(
                &mut self,
                asset_manager: &AssetManager,
                imgui_draw_data: &imgui::DrawData,
        ) -> AnyResult<()> {
                let frame_data = &self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;

                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.swapchain.resolve_imgs[0],
                                old_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                src_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                                dst_access_mask: vk::AccessFlags2::SHADER_READ,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.swapchain.resolve_imgs[1],
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                dst_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                {
                        let color_attachment = vk::RenderingAttachmentInfo::default()
                                .image_view(*self.swapchain.resolve_img_views[1])
                                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                                .load_op(vk::AttachmentLoadOp::CLEAR)
                                .store_op(vk::AttachmentStoreOp::STORE);

                        let rendering_info = vk::RenderingInfo::default()
                                .render_area(self.swapchain.scissor)
                                .layer_count(1)
                                .color_attachments(color_attachment.ref_into_slice());

                        self.context.device.cmd_begin_rendering(cmd_buffer, &rendering_info);
                }

                let hdr_shader_id = asset_manager.shader_names()["hdr-shader"];
                let hdr_vk_shader = &self.vk_asset_manager.graphics_shader(hdr_shader_id);
                let hdr_vk_pipeline = self.vk_asset_manager.get_pipeline_for_graphics_shader(
                        &self.context,
                        asset_manager,
                        hdr_shader_id,
                        self.swapchain.color_format,
                        vk::Format::UNDEFINED,
                )?;

                self.context
                        .device
                        .cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, hdr_vk_pipeline);

                // We dont use self.swapchain.viewport because it is upside down (we dont flip Y for this pass)
                let viewport = vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: self.swapchain.extent.width as f32,
                        height: self.swapchain.extent.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                self.context
                        .device
                        .cmd_set_viewport(cmd_buffer, 0, viewport.ref_into_slice());

                self.context
                        .device
                        .cmd_set_scissor(cmd_buffer, 0, self.swapchain.scissor.ref_into_slice());

                self.vk_asset_manager
                        .update_world_dst_set_for_graphics(&self.context, hdr_vk_shader, self.framei);

                self.context.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *hdr_vk_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[hdr_vk_shader.world_dst_set[self.framei]],
                        &[],
                );

                self.context.device.cmd_draw(cmd_buffer, 3, 1, 0, 0);

                self.imgui_renderer
                        .as_mut()
                        .unwrap()
                        .cmd_draw(cmd_buffer, imgui_draw_data)?;

                self.context.device.cmd_end_rendering(cmd_buffer);

                Ok(())
        }

        unsafe fn end_frame(&mut self, imagei: u32) -> AnyResult<()> {
                let frame_data = &mut self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;
                let present_img_data = &self.swapchain.present_imgs[imagei as usize];

                /* Prepare resolve image for copying from */
                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.swapchain.resolve_imgs[1],
                                old_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                src_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_READ,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                /* Prepare present image for copying into */
                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: present_img_data.img,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                VkImage::cmd_copy_image_to_image(
                        &self.context.device,
                        cmd_buffer,
                        self.swapchain.extent.width,
                        self.swapchain.extent.height,
                        *self.swapchain.resolve_imgs[1],
                        present_img_data.img,
                        vk::Filter::NEAREST,
                );

                // Prepare swapchain image for presentation
                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: present_img_data.img,
                                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                                src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::NONE,
                                dst_access_mask: vk::AccessFlags2::NONE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                self.context.device.end_command_buffer(cmd_buffer)?;

                {
                        let wait_semaphore_info = vk::SemaphoreSubmitInfo::default()
                                .semaphore(*frame_data.img_available_semaphore)
                                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS);

                        let command_buffer_info = vk::CommandBufferSubmitInfo::default().command_buffer(cmd_buffer);

                        let signal_semaphore_info = vk::SemaphoreSubmitInfo::default()
                                .semaphore(*present_img_data.render_finished_semaphore);

                        let submit_info = vk::SubmitInfo2::default()
                                .wait_semaphore_infos(wait_semaphore_info.ref_into_slice())
                                .command_buffer_infos(command_buffer_info.ref_into_slice())
                                .signal_semaphore_infos(signal_semaphore_info.ref_into_slice());

                        self.context.device.queue_submit2(
                                self.context.queues.graphics,
                                &[submit_info],
                                *frame_data.draw_cmd_buffer.fence,
                        )?;
                }

                match self.swapchain.queue_present(
                        self.context.queues.present,
                        &vk::PresentInfoKHR::default()
                                .wait_semaphores(&[*present_img_data.render_finished_semaphore])
                                .swapchains(&[*self.swapchain])
                                .image_indices(&[imagei]),
                ) {
                        Ok(suboptimal) if suboptimal => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauseFlags::SUBOPTIMAL);
                        },
                        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                                self.swapchain_outdated_causes
                                        .insert(VkSwapchainOutdatedCauseFlags::OUT_OF_DATE);
                        },
                        Err(err) => return Err(err.into()),
                        _ => (),
                };

                self.framei = (self.framei + 1) % self.max_concurrent_frames;

                Ok(())
        }

        fn process_world_model_instances(
                world: &mut World,
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                framei: usize,
                world_matrices: &WorldMatrices,
                buffer_transform_idx: &mut usize,
                scene: &mut VkRenderScene,
        ) -> VkResult<()> {
                // TODO: actually handle dynamic uniform buffer bindings correctly

                for (minstance, transform) in world.query::<(&ModelInstance, &InterpGlobalTransform)>().iter(world) {
                        Self::process_model_instance(
                                asset_manager,
                                vk_asset_manager,
                                framei,
                                world_matrices,
                                minstance,
                                transform.0.as_matrix(),
                                *buffer_transform_idx,
                                scene,
                        )?;

                        *buffer_transform_idx += 1;
                }

                Ok(())
        }

        fn process_model_instance(
                asset_manager: &AssetManager,
                vk_asset_manager: &VkAssetManager,
                framei: usize,
                world_matrices: &WorldMatrices,
                minstance: &ModelInstance,
                model_matrix: Mat4,
                buffer_transform_idx: usize,
                scene: &mut VkRenderScene,
        ) -> VkResult<()> {
                let object_matrices_offset = {
                        let model = model_matrix;
                        let mvp = world_matrices.vp * model;
                        let normal = model.inverse().transpose();

                        let object_matrices = ObjectMatrices { model, mvp, normal };

                        let object_matrices_buffer = &vk_asset_manager
                                .shader_resource_dynamic_buffers
                                .get(&SHADER_RESOURCE_OBJECT_MATRICES)
                                .unwrap()[framei];

                        object_matrices_buffer.write(&object_matrices, buffer_transform_idx)?
                };

                let model = asset_manager.model(minstance.model);

                for material_mesh in &model.meshes {
                        Self::process_mesh_instance(asset_manager, material_mesh, object_matrices_offset, scene);
                }

                Ok(())
        }

        fn process_mesh_instance(
                asset_manager: &AssetManager,
                material_mesh: &MaterialMesh,
                object_matrices_dynamic_offset: usize,
                scene: &mut VkRenderScene,
        ) {
                let material = asset_manager.material(material_mesh.material);

                let material_mesh_instances = scene
                        .mesh_instances
                        .entry(material.shader)
                        .unwrap()
                        .or_insert_with(|| SecondaryMap::new());

                let mesh_instances = material_mesh_instances
                        .entry(material_mesh.material)
                        .unwrap()
                        .or_insert_with(|| vec![]);

                mesh_instances.push(VkMeshInstance {
                        mesh: material_mesh.mesh,
                        object_matrices_dynamic_offset,
                });
        }

        unsafe fn map_point_shadows(&self, asset_manager: &AssetManager, scene: &VkRenderScene) -> VkResult<()> {
                let frame_data = &self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;

                let color_clear_value = vk::ClearValue {
                        color: vk::ClearColorValue {
                                float32: [f32::MAX, 0.0, 0.0, 0.0], // We only care about red channel
                        },
                };

                let depth_clear_value = vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                };

                let shadow_map_rect = vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                                width: SHADOW_MAP_WIDTH,
                                height: SHADOW_MAP_HEIGHT,
                        },
                };

                // NOTE: we don't negate the height because otherwise cubemap addressing is a pain.
                let shadow_map_viewport = vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: SHADOW_MAP_WIDTH as f32,
                        height: SHADOW_MAP_HEIGHT as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.cube_shadow_map_img,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                dst_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.cube_shadow_map_depth_img,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                                        | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                                dst_access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_depth(),
                        },
                );

                let cube_shadow_map_shader_id = asset_manager.shader_names()["cube-shadow-map"];
                let cube_shadow_map_shader = &self.vk_asset_manager.graphics_shader(cube_shadow_map_shader_id);

                self.vk_asset_manager.update_world_dst_set_for_graphics(
                        &self.context,
                        cube_shadow_map_shader,
                        self.framei,
                );

                for i in 0..6 {
                        {
                                let color_attachment = vk::RenderingAttachmentInfo::default()
                                        .image_view(*self.cube_shadow_map_img_views[i])
                                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                                        .load_op(vk::AttachmentLoadOp::CLEAR)
                                        .store_op(vk::AttachmentStoreOp::STORE)
                                        .clear_value(color_clear_value);

                                let depth_attachment = vk::RenderingAttachmentInfo::default()
                                        .image_view(*self.cube_shadow_map_depth_img_view)
                                        .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                                        .load_op(vk::AttachmentLoadOp::CLEAR)
                                        .store_op(vk::AttachmentStoreOp::DONT_CARE)
                                        .clear_value(depth_clear_value);

                                let rendering_info = vk::RenderingInfo::default()
                                        .render_area(shadow_map_rect)
                                        .layer_count(1)
                                        .color_attachments(color_attachment.ref_into_slice())
                                        .depth_attachment(&depth_attachment);

                                self.context.device.cmd_begin_rendering(cmd_buffer, &rendering_info);
                        }

                        self.context
                                .device
                                .cmd_set_viewport(cmd_buffer, 0, slice::from_ref(&shadow_map_viewport));

                        self.context
                                .device
                                .cmd_set_scissor(cmd_buffer, 0, slice::from_ref(&shadow_map_rect));

                        let cube_shadow_map_pipeline = self.vk_asset_manager.get_pipeline_for_graphics_shader(
                                &self.context,
                                asset_manager,
                                cube_shadow_map_shader_id,
                                self.cube_shadow_map_img.format,
                                self.cube_shadow_map_depth_img.format,
                        )?;

                        self.context.device.cmd_bind_pipeline(
                                cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                cube_shadow_map_pipeline,
                        );

                        self.context.device.cmd_push_constants(
                                cmd_buffer,
                                *cube_shadow_map_shader.graphics_pipeline_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                (i as u32).as_bytes(),
                        );

                        self.context.device.cmd_bind_descriptor_sets(
                                cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *cube_shadow_map_shader.graphics_pipeline_layout,
                                VkDescriptorSetIndex::World.value(),
                                &[cube_shadow_map_shader.world_dst_set[self.framei]],
                                &[],
                        );

                        for (_, shader_group) in &scene.mesh_instances {
                                for (_, material_group) in shader_group {
                                        for mesh_instance in material_group {
                                                self.draw_mesh_instance(
                                                        cmd_buffer,
                                                        cube_shadow_map_shader,
                                                        mesh_instance,
                                                );
                                        }
                                }
                        }

                        self.context.device.cmd_end_rendering(cmd_buffer);
                }

                // Prepare cube shadow map image for reading in shader.
                // TODO: should this be done in draw_scene()?
                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.cube_shadow_map_img,
                                old_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                                src_access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                                dst_access_mask: vk::AccessFlags2::SHADER_READ,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                Ok(())
        }

        unsafe fn map_shadows(&self, asset_manager: &AssetManager, scene: &VkRenderScene) -> VkResult<()> {
                let frame_data = &self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;

                let depth_clear_value = vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                };

                let shadow_map_rect = vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                                width: SHADOW_MAP_WIDTH,
                                height: SHADOW_MAP_HEIGHT,
                        },
                };

                // NOTE: we negate the height so that the depth map is not upside down. Because of this,
                // in the fragment shader we have to flip the y coordinate of textures. If we didn't negate
                // the height, we could skip that step in the fragment shader, but then the cull mode would
                // be "reversed", i.e., front cull would actually mean back cull and viceversa, which is kinda confusing.
                let shadow_map_viewport = vk::Viewport {
                        x: 0.0,
                        y: SHADOW_MAP_HEIGHT as f32,
                        width: SHADOW_MAP_WIDTH as f32,
                        height: -(SHADOW_MAP_HEIGHT as f32), // flip to change coordinate system
                        min_depth: 0.0,
                        max_depth: 1.0,
                };

                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.shadow_map_depth_img,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                                        | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                                dst_access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_depth(),
                        },
                );

                {
                        let depth_attachment = vk::RenderingAttachmentInfo::default()
                                .image_view(*self.shadow_map_depth_img_view)
                                .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                                .load_op(vk::AttachmentLoadOp::CLEAR)
                                .store_op(vk::AttachmentStoreOp::STORE)
                                .clear_value(depth_clear_value);

                        let rendering_info = vk::RenderingInfo::default()
                                .render_area(shadow_map_rect)
                                .layer_count(1)
                                .depth_attachment(&depth_attachment);

                        self.context.device.cmd_begin_rendering(cmd_buffer, &rendering_info);
                }

                self.context
                        .device
                        .cmd_set_viewport(cmd_buffer, 0, slice::from_ref(&shadow_map_viewport));

                self.context
                        .device
                        .cmd_set_scissor(cmd_buffer, 0, slice::from_ref(&shadow_map_rect));

                let shadow_map_shader_id = asset_manager.shader_names()["shadow-map"];
                let shadow_map_shader = &self.vk_asset_manager.graphics_shader(shadow_map_shader_id);
                let shadow_map_pipeline = self.vk_asset_manager.get_pipeline_for_graphics_shader(
                        &self.context,
                        asset_manager,
                        shadow_map_shader_id,
                        vk::Format::UNDEFINED,
                        self.shadow_map_depth_img.format,
                )?;

                self.vk_asset_manager
                        .update_world_dst_set_for_graphics(&self.context, shadow_map_shader, self.framei);

                self.context
                        .device
                        .cmd_bind_pipeline(cmd_buffer, vk::PipelineBindPoint::GRAPHICS, shadow_map_pipeline);

                self.context.device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *shadow_map_shader.graphics_pipeline_layout,
                        VkDescriptorSetIndex::World.value(),
                        &[shadow_map_shader.world_dst_set[self.framei]],
                        &[],
                );

                for (_, shader_group) in &scene.mesh_instances {
                        for (_, material_group) in shader_group {
                                for mesh_instance in material_group {
                                        self.draw_mesh_instance(cmd_buffer, shadow_map_shader, mesh_instance);
                                }
                        }
                }

                self.context.device.cmd_end_rendering(cmd_buffer);

                // TODO: should this be done in draw_scene()?
                VkImage::cmd_transition_img_layout(
                        &self.context.device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *self.shadow_map_depth_img,
                                old_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                                src_access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                                dst_access_mask: vk::AccessFlags2::SHADER_READ,
                                subresource_range: vk::ImageSubresourceRange::full_depth(),
                        },
                );

                Ok(())
        }

        fn update_skybox(
                asset_manager: &AssetManager,
                vk_asset_manager: &mut VkAssetManager,
                cubemap_id: CubemapId,
                framei: usize,
        ) -> VkResult<()> {
                let vk_skybox = &vk_asset_manager.cubemaps[cubemap_id];

                let environment_image_view = vk_skybox.environment_image_view.handle();
                let environment_sampler = vk_skybox.environment_sampler.handle();
                let irradiance_image_view = vk_skybox.irradiance_image_view.handle();
                let irradiance_sampler = vk_skybox.irradiance_sampler.handle();
                let prefiltered_image_view = vk_skybox.prefiltered_image_view.handle();
                let prefiltered_sampler = vk_skybox.prefiltered_sampler.handle();

                vk_asset_manager.provide_texture_shader_resource_directly(
                        asset_manager,
                        &SHADER_RESOURCE_SKYBOX,
                        environment_image_view,
                        environment_sampler,
                        framei,
                )?;

                vk_asset_manager.provide_texture_shader_resource_directly(
                        asset_manager,
                        &SHADER_RESOURCE_IRRADIANCE_MAP,
                        irradiance_image_view,
                        irradiance_sampler,
                        framei,
                )?;

                vk_asset_manager.provide_texture_shader_resource_directly(
                        asset_manager,
                        &SHADER_RESOURCE_PREFILTERED_MAP,
                        prefiltered_image_view,
                        prefiltered_sampler,
                        framei,
                )?;

                Ok(())
        }
}

struct VkFrameData {
        // Signaled when a swapchain image has become available for presentation.
        // This is needed because vkAcquireImage may return an image that is not immediately available.
        img_available_semaphore: VkSemaphore,
        // Command buffer used for submitting draw operations of one frame.
        draw_cmd_buffer: VkReusableCommandBuffer,
}

impl VkFrameData {
        fn new(context: &mut VkContext) -> AnyResult<Self> {
                let semaphore_cinfo = vk::SemaphoreCreateInfo::default();
                let img_available_semaphore = unsafe { VkSemaphore::new(&context.device, &semaphore_cinfo)? };

                let draw_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&context.device), Rc::clone(&context.cmd_pool))?;

                Ok(Self {
                        img_available_semaphore,
                        draw_cmd_buffer,
                })
        }
}

impl Drop for VkFrameData {
        fn drop(&mut self) {
                unsafe {
                        self.draw_cmd_buffer.destroy();
                        self.img_available_semaphore.destroy();
                }
        }
}

enum BeginFrameResult {
        Draw { imagei: u32 },
        Skip,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
// TODO: move out of this file (as it is not vulkan specific).
pub struct WorldMatrices {
        view_pos: Vec4,
        view: Mat4,
        proj: Mat4,
        vp: Mat4,
        inv_proj: Mat4,
        cluster_grid_size: Vec4u, // xyz is the cluster grid size, w is the total cluster count, i.e. x * y * z.
        near_far_viewport_size: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
struct WorldDirectionalLight {
        vp: Mat4,
        direction: Vec4,
        color_and_intensity: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
struct WorldPointLight {
        vp_mats: [Mat4; 6],
        pos: Vec4,
        color: Vec4,
        kc_kl_kq: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct WorldPointLightNoShadow {
        pos: Vec4,
        color: Vec4,
        kc_kl_kq: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
struct WorldSpotlight {
        pos: Vec4,
        dir: Vec4, // xyz=direction w=angle
        color: Vec4,
        kc_kl_kq_inner: Vec4, // w=inner radius percentage
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct WorldLights {
        dir_light: WorldDirectionalLight,
        point_light: WorldPointLight,
        spotlight: WorldSpotlight,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct MaterialData {
        pub ambient_color: Vec4,
        pub diffuse_color: Vec4,
        pub specular_color: Vec4,
        pub shininess_and_ambient_strength: Vec2,
        pub specular_strength_and_diffuse_strength: Vec2,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct BillboardData {
        billboard_center: Vec4,
        billboard_scale: Vec4,
        camera_right: Vec4,
        camera_up: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct ObjectMatrices {
        model: Mat4,
        mvp: Mat4,
        normal: Mat4,
}

#[allow(dead_code)]
#[repr(C)]
struct MatricesMMvp {
        model: Mat4,
        mvp: Mat4,
}

#[allow(dead_code)]
#[repr(C)]
struct UniformLights {
        light_pos: Vec4,
        light_color: Vec4,
}

struct VkRenderScene {
        skybox_object_matrices_offset: Option<usize>,
        mesh_instances: SecondaryMap<ShaderId, SecondaryMap<MaterialId, Vec<VkMeshInstance>>>,
}

struct VkMeshInstance {
        mesh: MeshId,
        object_matrices_dynamic_offset: usize,
}
