use ash::{prelude::VkResult, vk};
use slotmap::SecondaryMap;
use std::{rc::Rc, slice, time::Instant};

use bevy_ecs::prelude::World;
use crossbeam_channel::Receiver;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use shader_resource_derive::ShaderStruct;
use winit::{dpi::PhysicalSize, window::Window};

use super::{
        vk_asset_manager::{VkAssetManager, VkCubemap, VkDescriptorSetIndex, VkShader, VkShaderResourceType},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_context::VkContext,
        vk_image::{VkImage, VkImageCreateInfo},
        vk_swapchain::{VkSwapchain, VkSwapchainOutdatedCauseFlags},
        vk_util,
        vk_wrapper::{
                VkDevice, VkFramebuffer, VkImageView, VkInstance, VkRenderPass, VkSampler, VkSemaphore, VmaAllocator,
        },
};
use crate::{
        application::InterpGlobalTransform,
        asset_manager::{AssetManager, AssetManagerEvent, MaterialId, MaterialMesh, MeshId, ShaderId},
        components::{ActiveCamera, DirectionalLight, PointLight, ProjectionCamera, Spotlight},
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE, SHADOW_MAP_HEIGHT, SHADOW_MAP_WIDTH},
        hashmap::HashMap,
        model_instance_manager::ModelInstance,
        my_glm::*,
        renderer::Renderer,
        shader_resource::ShaderResourceId,
        shader_resources::{
                SHADER_RESOURCE_BILLBOARD_DATA, SHADER_RESOURCE_CUBE_SHADOW_MAP, SHADER_RESOURCE_MATERIAL_DATA,
                SHADER_RESOURCE_OBJECT_MATRICES, SHADER_RESOURCE_SHADOW_MAP, SHADER_RESOURCE_SKYBOX,
                SHADER_RESOURCE_WORLD_LIGHTS, SHADER_RESOURCE_WORLD_MATRICES,
        },
        skybox::Skybox,
        util::{RefIntoBytesSlice, RefIntoSlice},
};
use crate::{
        constants::{DESIRED_SWAPCHAIN_IMG_COUNT, MAX_CONCURRENT_FRAMES},
        util::DerefIntoSlice,
        AnyResult,
};

pub struct VkRenderer {
        window: Rc<Window>,
        asset_manager_event_rx: Receiver<AssetManagerEvent>,

        vk_context: VkContext,
        vk_asset_manager: VkAssetManager,

        swapchain: VkSwapchain,
        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags,

        cube_shadow_map_render_pass: VkRenderPass,
        shadow_map_render_pass: VkRenderPass,
        render_pass: VkRenderPass,

        shadow_map_img: VkImage,
        shadow_map_img_view: VkImageView,
        shadow_map_framebuffer: VkFramebuffer,
        shadow_map_sampler: VkSampler,

        cube_shadow_map_img: VkImage,
        cube_shadow_map_img_view: VkImageView, // For entire cube
        cube_shadow_map_img_views: [VkImageView; 6],
        cube_shadow_map_depth_img: VkImage,
        cube_shadow_map_depth_img_view: VkImageView,
        cube_shadow_map_framebuffers: [VkFramebuffer; 6],
        cube_shadow_map_sampler: VkSampler,

        setup_cmd_buffer: VkReusableCommandBuffer,

        max_concurrent_frames: usize,
        frames_data: Vec<VkFrameData>,

        imgui_renderer: Option<imgui_rs_vulkan_renderer::Renderer>,

        creation_instant: Instant,
        framei: usize,

        world_shader_resource_descriptors_data: HashMap<ShaderResourceId, ShaderResourceDescriptorData>,
}

impl VkRenderer {
        pub fn new(
                window: Rc<Window>,
                imguic: &mut imgui::Context,
                asset_manager_event_rx: Receiver<AssetManagerEvent>,
        ) -> AnyResult<Self> {
                let mut vk_context = VkContext::new(Rc::clone(&window))?;

                let mut swapchain = VkSwapchain::new(
                        Rc::clone(&window),
                        Rc::clone(&vk_context.instance),
                        Rc::clone(&vk_context.surface),
                        **vk_context.pdevice,
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
                        DESIRED_SWAPCHAIN_IMG_COUNT,
                )?;
                trace!("Created VkSwapchain");

                let render_pass = Self::create_render_pass(
                        &vk_context.device,
                        swapchain.samples,
                        swapchain.color_format.format,
                        swapchain.depth_format,
                )?;
                trace!("Created VkRenderPass");

                let shadow_map_depth_format = swapchain.depth_format;

                let shadow_map_render_pass =
                        Self::create_shadow_map_render_pass(&vk_context.device, shadow_map_depth_format)?;
                trace!("Created shadow map VkRenderPass");

                let cube_shadow_map_color_format =
                        Self::choose_cube_shadow_map_color_format(&vk_context.instance, **vk_context.pdevice)?;

                let cube_shadow_map_depth_format = shadow_map_depth_format;

                let cube_shadow_map_render_pass = Self::create_cube_shadow_map_render_pass(
                        &vk_context.device,
                        cube_shadow_map_color_format,
                        cube_shadow_map_depth_format,
                )?;
                trace!("Created shadow map VkRenderPass");

                swapchain.create_framebuffers(*render_pass)?;
                trace!("Created VkFramebuffers");

                let (shadow_map_img, shadow_map_img_view) = Self::create_shadow_map_img_and_view(
                        Rc::clone(&vk_context.device),
                        Rc::clone(&vk_context.allocator),
                        shadow_map_depth_format,
                )?;

                let shadow_map_framebuffer = Self::create_shadow_map_framebuffer(
                        &vk_context.device,
                        *shadow_map_render_pass,
                        *shadow_map_img_view,
                )?;

                let shadow_map_sampler = Self::create_shadow_map_sampler(&vk_context.device)?;

                let (cube_shadow_map_img, cube_shadow_map_img_view, cube_shadow_map_img_views) =
                        Self::create_cube_shadow_map_img_and_views(
                                &vk_context.device,
                                &vk_context.allocator,
                                cube_shadow_map_color_format,
                        )?;

                let (cube_shadow_map_depth_img, cube_shadow_map_depth_img_view) =
                        Self::create_cube_shadow_map_depth_img_and_view(
                                &vk_context.device,
                                &vk_context.allocator,
                                cube_shadow_map_depth_format,
                        )?;

                let cube_shadow_map_framebuffers = Self::create_cube_shadow_map_framebuffers(
                        &vk_context.device,
                        *cube_shadow_map_render_pass,
                        &cube_shadow_map_img_views,
                        *cube_shadow_map_depth_img_view,
                )?;

                let cube_shadow_map_sampler = Self::create_cube_shadow_map_sampler(&vk_context.device)?;

                let setup_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;
                trace!("Allocated VkCommandBuffers");

                let max_concurrent_frames = MAX_CONCURRENT_FRAMES;
                let frames_data = (0..max_concurrent_frames)
                        .map(|_| VkFrameData::new(&mut vk_context))
                        .collect::<AnyResult<Vec<VkFrameData>>>()?;

                let vk_asset_manager = VkAssetManager::new(
                        &mut vk_context,
                        swapchain.samples,
                        *render_pass,
                        *cube_shadow_map_render_pass,
                        *shadow_map_render_pass,
                        max_concurrent_frames,
                )?;
                trace!("Created VkAssetManager");

                let imgui_renderer_options = imgui_rs_vulkan_renderer::Options {
                        in_flight_frames: max_concurrent_frames,
                        enable_depth_test: false,
                        enable_depth_write: false,
                        sample_count: swapchain.samples,
                };

                let imgui_renderer = Some(imgui_rs_vulkan_renderer::Renderer::with_default_allocator(
                        &**vk_context.instance,
                        **vk_context.pdevice,
                        (**vk_context.device).clone(),
                        vk_context.queues.graphics,
                        **vk_context.cmd_pool,
                        *render_pass,
                        imguic,
                        Some(imgui_renderer_options),
                )?);

                Ok(Self {
                        window,
                        asset_manager_event_rx,

                        vk_context,
                        vk_asset_manager,

                        swapchain,
                        swapchain_outdated_causes: VkSwapchainOutdatedCauseFlags::NONE,

                        cube_shadow_map_render_pass,
                        shadow_map_render_pass,
                        render_pass,

                        shadow_map_img,
                        shadow_map_img_view,
                        shadow_map_framebuffer,
                        shadow_map_sampler,

                        cube_shadow_map_img,
                        cube_shadow_map_img_view,
                        cube_shadow_map_img_views,
                        cube_shadow_map_depth_img,
                        cube_shadow_map_depth_img_view,
                        cube_shadow_map_framebuffers,
                        cube_shadow_map_sampler,

                        setup_cmd_buffer,
                        max_concurrent_frames,
                        frames_data,

                        imgui_renderer,

                        creation_instant: Instant::now(),

                        framei: 0,

                        world_shader_resource_descriptors_data: HashMap::new(),
                })
        }
}

impl Renderer for VkRenderer {
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()> {
                self.vk_asset_manager.process_asset_manager_events(
                        world.get_resource::<AssetManager>().unwrap(),
                        &self.asset_manager_event_rx,
                )?;

                if !self.should_render() {
                        return Ok(());
                }

                let imagei = match unsafe { self.begin_frame()? } {
                        BeginFrameResult::Draw { imagei } => imagei,
                        BeginFrameResult::Skip => return Ok(()),
                };

                let frame_data = &self.frames_data[self.framei];

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
                };

                Self::write_struct_resource(
                        self.framei,
                        &self.vk_asset_manager,
                        &SHADER_RESOURCE_WORLD_MATRICES,
                        &world_matrices,
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
                        color: Vec4::from((dir_light_component.color, 1.0)),
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

                Self::write_struct_resource(
                        self.framei,
                        &self.vk_asset_manager,
                        &SHADER_RESOURCE_WORLD_LIGHTS,
                        &world_lights,
                )?;

                let camera_right = Vec4::from((camera_orien * Vec3::RIGHT, 0.0));
                let camera_up = Vec4::from((camera_orien * Vec3::UP, 0.0));

                let billboard_data = BillboardData {
                        billboard_center: Vec4::ZERO,
                        billboard_scale: Vec4::splat(0.5),
                        camera_right,
                        camera_up,
                };

                Self::write_struct_resource(
                        self.framei,
                        &self.vk_asset_manager,
                        &SHADER_RESOURCE_BILLBOARD_DATA,
                        &billboard_data,
                )?;

                let asset_manager = world.remove_resource::<AssetManager>().unwrap();

                let mut vk_render_scene = VkRenderScene {
                        skybox_object_matrices_offset: None,
                        mesh_instances: SecondaryMap::new(),
                };

                let mut buffer_transform_idx = 0;

                if let Some(skybox) = world.get_resource::<Skybox>() {
                        let vk_skybox = &self.vk_asset_manager.cubemaps[skybox.0];
                        Self::update_skybox(
                                &self.vk_asset_manager,
                                &mut self.world_shader_resource_descriptors_data,
                                vk_skybox,
                        );

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

                Self::write_image_resource(
                        &self.vk_asset_manager,
                        &SHADER_RESOURCE_SHADOW_MAP,
                        *self.shadow_map_img_view,
                        *self.shadow_map_sampler,
                        &mut self.world_shader_resource_descriptors_data,
                );

                Self::write_image_resource(
                        &self.vk_asset_manager,
                        &SHADER_RESOURCE_CUBE_SHADOW_MAP,
                        *self.cube_shadow_map_img_view,
                        *self.cube_shadow_map_sampler,
                        &mut self.world_shader_resource_descriptors_data,
                );

                unsafe {
                        self.map_point_shadows(&asset_manager, &vk_render_scene)?;
                        self.map_shadows(&asset_manager, &vk_render_scene)?;

                        let time = self.creation_instant.elapsed().as_secs_f32();
                        let intensity = (((time.sin() + 1.0) / 2.0) * 0.05) + 0.05;

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

                        let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                .render_pass(*self.render_pass)
                                .framebuffer(*self.swapchain.framebuffers[imagei as usize])
                                .render_area(self.swapchain.scissor)
                                .clear_values(&clear_values);

                        self.vk_context.device.cmd_begin_render_pass(
                                *frame_data.draw_cmd_buffer,
                                &render_pass_binfo,
                                vk::SubpassContents::INLINE,
                        );

                        self.vk_context.device.cmd_set_viewport(
                                *frame_data.draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.viewport),
                        );
                        self.vk_context.device.cmd_set_scissor(
                                *frame_data.draw_cmd_buffer,
                                0,
                                slice::from_ref(&self.swapchain.scissor),
                        );

                        self.draw_scene(*frame_data.draw_cmd_buffer, &asset_manager, &vk_render_scene)?;

                        self.imgui_renderer
                                .as_mut()
                                .unwrap()
                                .cmd_draw(*frame_data.draw_cmd_buffer, imgui_draw_data)?;

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
                        let _ = self.vk_context.device.device_wait_idle();
                        drop(self.imgui_renderer.take().unwrap());
                        self.frames_data.clear();
                        self.cube_shadow_map_sampler.destroy();
                        self.cube_shadow_map_framebuffers.iter().for_each(|x| x.destroy());
                        self.cube_shadow_map_depth_img_view.destroy();
                        self.cube_shadow_map_depth_img.destroy();
                        self.cube_shadow_map_img_views.iter().for_each(|x| x.destroy());
                        self.cube_shadow_map_img_view.destroy();
                        self.cube_shadow_map_img.destroy();
                        self.setup_cmd_buffer.destroy();
                        self.shadow_map_sampler.destroy();
                        self.shadow_map_framebuffer.destroy();
                        self.shadow_map_img_view.destroy();
                        self.shadow_map_img.destroy();
                        self.shadow_map_render_pass.destroy();
                        self.cube_shadow_map_render_pass.destroy();
                        self.render_pass.destroy();
                        self.swapchain.destroy();
                        self.vk_asset_manager.destroy();
                        self.vk_context.destroy();
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

                unsafe { self.vk_context.device.device_wait_idle()? };

                let srecreation_info = self.swapchain.recreate()?;

                let mut recreate_render_pass: bool = false;
                let mut recreate_pipeline: bool = false;

                if srecreation_info.color_format_changed || srecreation_info.samples_changed {
                        recreate_render_pass = true;
                }

                if recreate_render_pass {
                        trace!("Recreating VkRenderPass...");

                        self.render_pass = Self::create_render_pass(
                                &self.vk_context.device,
                                self.swapchain.samples,
                                self.swapchain.color_format.format,
                                self.swapchain.depth_format,
                        )?;

                        self.imgui_renderer
                                .as_mut()
                                .unwrap()
                                .set_render_pass(*self.render_pass)?;

                        recreate_pipeline = true;
                }

                self.swapchain.create_framebuffers(*self.render_pass)?;

                if recreate_pipeline {
                        trace!("Recreating VkGraphicsPipeline...");
                        // TODO: recreate pipelines in VkAssetManager
                        // let new_graphics_pipeline = Self::create_graphics_pipeline(
                        //         &self.vk_asset_manager.shaders[self.basic_shader_id],
                        //         &self.vk_context.device,
                        //         self.swapchain.samples,
                        //         *self.render_pass,
                        //         &self.graphics_pipeline_layout,
                        // )?;
                        // let old_graphics_pipeline =
                        // std::mem::replace(&mut self.graphics_pipeline, new_graphics_pipeline);
                        // unsafe { old_graphics_pipeline.destroy() };
                }

                self.swapchain_outdated_causes = VkSwapchainOutdatedCauseFlags::NONE;

                Ok(())
        }

        fn create_render_pass(
                device: &Rc<VkDevice>,
                swch_samples: vk::SampleCountFlags,
                swch_color_format: vk::Format,
                swch_depth_format: vk::Format,
        ) -> VkResult<VkRenderPass> {
                let attachments = [
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_color_format,
                                samples: swch_samples,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::DONT_CARE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_depth_format,
                                samples: swch_samples,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::DONT_CARE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: swch_color_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::DONT_CARE,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
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

                let resolve_attachment_ref = vk::AttachmentReference {
                        attachment: 2,
                        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                };

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        //.input_attachments(&[])
                        .color_attachments(color_attachment_ref.ref_into_slice())
                        .depth_stencil_attachment(&depth_attachment_ref)
                        .resolve_attachments(resolve_attachment_ref.ref_into_slice())
                        //.preserve_attachments(&[])
                        .build()];

                // Alternative that also works:
                // let subpass_dependencies = [
                //         vk::SubpassDependency {
                //                 src_subpass: vk::SUBPASS_EXTERNAL,
                //                 dst_subpass: 0,
                //                 src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                //                         | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                //                 dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                //                         | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                //                 src_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                //                         | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                //                 dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                //                         | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                //                 dependency_flags: vk::DependencyFlags::empty(),
                //         },
                // ];

                let subpass_dependencies = [
                        vk::SubpassDependency {
                                src_subpass: vk::SUBPASS_EXTERNAL,
                                dst_subpass: 0,
                                src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                src_access_mask: vk::AccessFlags::NONE,
                                dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                        vk::SubpassDependency {
                                src_subpass: 0,
                                dst_subpass: vk::SUBPASS_EXTERNAL,
                                src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                src_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                                dst_access_mask: vk::AccessFlags::NONE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                        vk::SubpassDependency {
                                src_subpass: vk::SUBPASS_EXTERNAL,
                                dst_subpass: 0,
                                src_stage_mask: vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                dst_stage_mask: vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                                src_access_mask: vk::AccessFlags::NONE,
                                dst_access_mask: vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                        vk::SubpassDependency {
                                src_subpass: 0,
                                dst_subpass: vk::SUBPASS_EXTERNAL,
                                src_stage_mask: vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                dst_stage_mask: vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                                src_access_mask: vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dst_access_mask: vk::AccessFlags::NONE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                ];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { VkRenderPass::new(device, &render_pass_cinfo) }
        }

        fn create_shadow_map_render_pass(device: &Rc<VkDevice>, depth_format: vk::Format) -> VkResult<VkRenderPass> {
                let attachments = [vk::AttachmentDescription {
                        flags: vk::AttachmentDescriptionFlags::empty(),
                        format: depth_format,
                        samples: vk::SampleCountFlags::TYPE_1,
                        load_op: vk::AttachmentLoadOp::CLEAR,
                        store_op: vk::AttachmentStoreOp::STORE,
                        stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                        stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        final_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                }];

                let depth_attachment_ref = vk::AttachmentReference {
                        attachment: 0,
                        layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                };

                let subpass_descriptions = [vk::SubpassDescription::builder()
                        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                        //.input_attachments(&[])
                        // .color_attachments(&[])
                        .depth_stencil_attachment(&depth_attachment_ref)
                        // .resolve_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];

                let subpass_dependencies = [
                        vk::SubpassDependency {
                                src_subpass: vk::SUBPASS_EXTERNAL,
                                dst_subpass: 0,
                                src_stage_mask: vk::PipelineStageFlags::FRAGMENT_SHADER,
                                dst_stage_mask: vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                                src_access_mask: vk::AccessFlags::NONE,
                                dst_access_mask: vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                        vk::SubpassDependency {
                                src_subpass: 0,
                                dst_subpass: vk::SUBPASS_EXTERNAL,
                                src_stage_mask: vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                dst_stage_mask: vk::PipelineStageFlags::FRAGMENT_SHADER,
                                src_access_mask: vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dst_access_mask: vk::AccessFlags::SHADER_READ,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                ];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { VkRenderPass::new(device, &render_pass_cinfo) }
        }

        fn create_cube_shadow_map_render_pass(
                device: &Rc<VkDevice>,
                cube_shadow_map_color_format: vk::Format,
                cube_shadow_map_depth_format: vk::Format,
        ) -> VkResult<VkRenderPass> {
                let attachments = [
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: cube_shadow_map_color_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
                                stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                final_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        },
                        vk::AttachmentDescription {
                                flags: vk::AttachmentDescriptionFlags::empty(),
                                format: cube_shadow_map_depth_format,
                                samples: vk::SampleCountFlags::TYPE_1,
                                load_op: vk::AttachmentLoadOp::CLEAR,
                                store_op: vk::AttachmentStoreOp::DONT_CARE,
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
                        //.input_attachments(&[])
                        .color_attachments(&[color_attachment_ref])
                        .depth_stencil_attachment(&depth_attachment_ref)
                        //.resolve_attachments(&[])
                        //.preserve_attachments(&[])
                        .build()];

                let subpass_dependencies = [
                        vk::SubpassDependency {
                                src_subpass: vk::SUBPASS_EXTERNAL,
                                dst_subpass: 0,
                                src_stage_mask: vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                                dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                                src_access_mask: vk::AccessFlags::MEMORY_WRITE,
                                dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                        vk::SubpassDependency {
                                src_subpass: 0,
                                dst_subpass: vk::SUBPASS_EXTERNAL,
                                src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                                dst_stage_mask: vk::PipelineStageFlags::FRAGMENT_SHADER,
                                src_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                                dst_access_mask: vk::AccessFlags::SHADER_READ,
                                dependency_flags: vk::DependencyFlags::empty(),
                        },
                ];

                let render_pass_cinfo = vk::RenderPassCreateInfo::builder()
                        .attachments(&attachments)
                        .subpasses(&subpass_descriptions)
                        .dependencies(&subpass_dependencies);

                unsafe { VkRenderPass::new(device, &render_pass_cinfo) }
        }

        fn create_shadow_map_img_and_view(
                device: Rc<VkDevice>,
                allocator: Rc<VmaAllocator>,
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

                        VkImage::new(allocator, &img_cinfo)?
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

                        VkImageView::new(device, &img_view_cinfo)?
                };

                Ok((img, img_view))
        }

        fn create_shadow_map_framebuffer(
                device: &Rc<VkDevice>,
                shadow_map_render_pass: vk::RenderPass,
                shadow_map_img_view: vk::ImageView,
        ) -> VkResult<VkFramebuffer> {
                let attachments = [shadow_map_img_view];

                let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                        .render_pass(shadow_map_render_pass)
                        .attachments(&attachments)
                        .width(SHADOW_MAP_WIDTH)
                        .height(SHADOW_MAP_HEIGHT)
                        .layers(1);

                unsafe { VkFramebuffer::new(device, &framebuffer_cinfo) }
        }

        fn create_shadow_map_sampler(device: &Rc<VkDevice>) -> VkResult<VkSampler> {
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

                Ok(unsafe { VkSampler::new(Rc::clone(device), &vk_sampler_cinfo)? })
        }

        fn choose_cube_shadow_map_color_format(
                instance: &VkInstance,
                pdevice: vk::PhysicalDevice,
        ) -> VkResult<vk::Format> {
                let candidates = [vk::Format::R32_SFLOAT, vk::Format::R16_SFLOAT];

                let features = vk::FormatFeatureFlags::COLOR_ATTACHMENT;

                vk_util::find_best_format_for_optimal_tiling(instance, pdevice, &candidates, features)
        }

        fn create_cube_shadow_map_img_and_views(
                device: &Rc<VkDevice>,
                allocator: &Rc<VmaAllocator>,
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

                        VkImage::new(Rc::clone(allocator), &img_cinfo)?
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

                let img_view = unsafe { VkImageView::new(Rc::clone(device), &img_view_cinfo)? };

                img_view_cinfo.view_type = vk::ImageViewType::TYPE_2D;
                img_view_cinfo.subresource_range.layer_count = 1;
                let mut mk_img_view = |l| unsafe {
                        img_view_cinfo.subresource_range.base_array_layer = l;
                        VkImageView::new(Rc::clone(device), &img_view_cinfo)
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
                device: &Rc<VkDevice>,
                allocator: &Rc<VmaAllocator>,
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

                        VkImage::new(Rc::clone(allocator), &img_cinfo)?
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

                let img_view = unsafe { VkImageView::new(Rc::clone(device), &img_view_cinfo)? };

                Ok((img, img_view))
        }

        fn create_cube_shadow_map_framebuffers(
                device: &Rc<VkDevice>,
                cube_shadow_map_render_pass: vk::RenderPass,
                cube_shadow_map_color_img_views: &[VkImageView; 6],
                cube_shadow_map_depth_img_view: vk::ImageView,
        ) -> VkResult<[VkFramebuffer; 6]> {
                let mk_framebuffer = |i: usize| {
                        let attachments = [*cube_shadow_map_color_img_views[i], cube_shadow_map_depth_img_view];

                        let framebuffer_cinfo = vk::FramebufferCreateInfo::builder()
                                .render_pass(cube_shadow_map_render_pass)
                                .width(SHADOW_MAP_WIDTH)
                                .height(SHADOW_MAP_HEIGHT)
                                .layers(1)
                                .attachments(&attachments);

                        unsafe { VkFramebuffer::new(device, &framebuffer_cinfo) }
                };

                Ok([
                        mk_framebuffer(0)?,
                        mk_framebuffer(1)?,
                        mk_framebuffer(2)?,
                        mk_framebuffer(3)?,
                        mk_framebuffer(4)?,
                        mk_framebuffer(5)?,
                ])
        }

        fn create_cube_shadow_map_sampler(device: &Rc<VkDevice>) -> VkResult<VkSampler> {
                Self::create_shadow_map_sampler(device)
        }

        // TODO: check that T is compatible with the shader resource type.
        fn write_struct_resource<T: 'static>(
                framei: usize,
                vk_asset_manager: &VkAssetManager,
                resource_id: &ShaderResourceId,
                data: &T,
        ) -> AnyResult<()> {
                let vk_resource = vk_asset_manager.shader_resources.get(resource_id).unwrap();

                match vk_resource.resource_type {
                        VkShaderResourceType::UniformBuffer => {
                                let buffers = vk_asset_manager.shader_resource_buffers.get(resource_id).unwrap();
                                let buffer = &buffers[framei];

                                buffer.write(data)?;
                        },
                        VkShaderResourceType::UniformBufferDynamic => todo!(),
                        VkShaderResourceType::CombinedImageSampler => panic!(),
                }

                Ok(())
        }

        fn write_image_resource(
                vk_asset_manager: &VkAssetManager,
                resource_id: &ShaderResourceId,
                image_view: vk::ImageView,
                sampler: vk::Sampler,
                world_shader_resource_descriptors_data: &mut HashMap<ShaderResourceId, ShaderResourceDescriptorData>,
        ) {
                let vk_resource = vk_asset_manager.shader_resources.get(resource_id).unwrap();

                match vk_resource.resource_type {
                        VkShaderResourceType::UniformBuffer => panic!(),
                        VkShaderResourceType::UniformBufferDynamic => panic!(),
                        VkShaderResourceType::CombinedImageSampler => {
                                let data = ShaderResourceDescriptorData::Image2D { image_view, sampler };

                                world_shader_resource_descriptors_data.insert(resource_id.clone(), data);
                        },
                }
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

                self.vk_context
                        .device
                        .wait_for_fences(&[*frame_data.draw_cmd_buffer.fence], true, u64::MAX)?;
                self.vk_context
                        .device
                        .reset_fences(&[*frame_data.draw_cmd_buffer.fence])?;

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

                self.vk_context.device.reset_command_buffer(
                        *frame_data.draw_cmd_buffer,
                        vk::CommandBufferResetFlags::RELEASE_RESOURCES,
                )?;

                let cmd_buffer_binfo =
                        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

                self.vk_context
                        .device
                        .begin_command_buffer(*frame_data.draw_cmd_buffer, &cmd_buffer_binfo)?;

                Ok(BeginFrameResult::Draw { imagei })
        }

        unsafe fn draw_scene(
                &self,
                cmd_buffer: vk::CommandBuffer,
                asset_manager: &AssetManager,
                scene: &VkRenderScene,
        ) -> VkResult<()> {
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

                        self.draw_shader_group(asset_manager, cmd_buffer, skybox_material.shader, &shader_group)?;
                }

                for (shader_id, shader_group) in &scene.mesh_instances {
                        self.draw_shader_group(asset_manager, cmd_buffer, shader_id, shader_group)?;
                }

                Ok(())
        }

        unsafe fn draw_shader_group(
                &self,
                asset_manager: &AssetManager,
                cmd_buffer: vk::CommandBuffer,
                shader_id: ShaderId,
                shader_group: &SecondaryMap<MaterialId, Vec<VkMeshInstance>>,
        ) -> VkResult<()> {
                let device = &*self.vk_context.device;
                let framei = self.framei;
                let vk_shader = &self.vk_asset_manager.shaders[shader_id];

                device.cmd_bind_pipeline(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *vk_shader.graphics_pipeline,
                );

                Self::update_world_descriptors(device, &self.world_shader_resource_descriptors_data, framei, vk_shader);

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
                vk_shader: &VkShader,
                material_id: MaterialId,
                material_group: &Vec<VkMeshInstance>,
        ) -> VkResult<()> {
                let device = &*self.vk_context.device;
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
                vk_shader: &VkShader,
                vk_mesh_instance: &VkMeshInstance,
        ) {
                let device = &*self.vk_context.device;
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

        unsafe fn end_frame(&mut self, imagei: u32) -> AnyResult<()> {
                let frame_data = &mut self.frames_data[self.framei];

                self.vk_context.device.cmd_end_render_pass(*frame_data.draw_cmd_buffer);
                self.vk_context.device.end_command_buffer(*frame_data.draw_cmd_buffer)?;

                let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
                let submit_info = vk::SubmitInfo::builder()
                        .command_buffers(frame_data.draw_cmd_buffer.deref_into_slice())
                        .wait_semaphores(frame_data.img_available_semaphore.deref_into_slice())
                        .wait_dst_stage_mask(&wait_stages)
                        .signal_semaphores(frame_data.render_finished_semaphore.deref_into_slice());

                self.vk_context.device.queue_submit(
                        self.vk_context.queues.graphics,
                        &[submit_info.build()],
                        *frame_data.draw_cmd_buffer.fence,
                )?;

                match self.swapchain.queue_present(
                        self.vk_context.queues.present,
                        &vk::PresentInfoKHR::builder()
                                .wait_semaphores(&[*frame_data.render_finished_semaphore])
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

                let clear_values = [
                        vk::ClearValue {
                                color: vk::ClearColorValue {
                                        float32: [f32::MAX, 0.0, 0.0, 0.0], // We only care about red channel
                                },
                        },
                        vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                        },
                ];

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

                for i in 0..6 {
                        let framebuffer = *self.cube_shadow_map_framebuffers[i];

                        let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                                .render_pass(*self.cube_shadow_map_render_pass)
                                .framebuffer(framebuffer)
                                .render_area(shadow_map_rect)
                                .clear_values(&clear_values);

                        self.vk_context.device.cmd_begin_render_pass(
                                cmd_buffer,
                                &render_pass_binfo,
                                vk::SubpassContents::INLINE,
                        );

                        self.vk_context
                                .device
                                .cmd_set_viewport(cmd_buffer, 0, slice::from_ref(&shadow_map_viewport));

                        self.vk_context
                                .device
                                .cmd_set_scissor(cmd_buffer, 0, slice::from_ref(&shadow_map_rect));

                        let cube_shadow_map_shader_id = asset_manager.shader_names()["cube-shadow-map"];
                        let cube_shadow_map_shader = &self.vk_asset_manager.shaders[cube_shadow_map_shader_id];

                        self.vk_context.device.cmd_bind_pipeline(
                                cmd_buffer,
                                vk::PipelineBindPoint::GRAPHICS,
                                *cube_shadow_map_shader.graphics_pipeline,
                        );

                        self.vk_context.device.cmd_push_constants(
                                cmd_buffer,
                                *cube_shadow_map_shader.graphics_pipeline_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                (i as u32).into_bytes_slice(),
                        );

                        self.vk_context.device.cmd_bind_descriptor_sets(
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

                        self.vk_context.device.cmd_end_render_pass(cmd_buffer);
                }

                Ok(())
        }

        unsafe fn map_shadows(&self, asset_manager: &AssetManager, scene: &VkRenderScene) -> VkResult<()> {
                let frame_data = &self.frames_data[self.framei];
                let cmd_buffer = *frame_data.draw_cmd_buffer;

                let clear_values = [vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                }];

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

                let render_pass_binfo = vk::RenderPassBeginInfo::builder()
                        .render_pass(*self.shadow_map_render_pass)
                        .framebuffer(*self.shadow_map_framebuffer)
                        .render_area(shadow_map_rect)
                        .clear_values(&clear_values);

                self.vk_context.device.cmd_begin_render_pass(
                        cmd_buffer,
                        &render_pass_binfo,
                        vk::SubpassContents::INLINE,
                );

                self.vk_context
                        .device
                        .cmd_set_viewport(cmd_buffer, 0, slice::from_ref(&shadow_map_viewport));

                self.vk_context
                        .device
                        .cmd_set_scissor(cmd_buffer, 0, slice::from_ref(&shadow_map_rect));

                let shadow_map_shader_id = asset_manager.shader_names()["shadow-map"];
                let shadow_map_shader = &self.vk_asset_manager.shaders[shadow_map_shader_id];

                self.vk_context.device.cmd_bind_pipeline(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        *shadow_map_shader.graphics_pipeline,
                );

                self.vk_context.device.cmd_bind_descriptor_sets(
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

                self.vk_context.device.cmd_end_render_pass(cmd_buffer);

                Ok(())
        }

        fn update_world_descriptors(
                device: &VkDevice,
                world_shader_resource_descriptors_data: &HashMap<ShaderResourceId, ShaderResourceDescriptorData>,
                framei: usize,
                vk_shader: &VkShader,
        ) {
                for (resource_id, binding) in &vk_shader.shader_resource_bindings {
                        if binding.set != VkDescriptorSetIndex::World {
                                continue;
                        }

                        if binding.descriptor_type == vk::DescriptorType::COMBINED_IMAGE_SAMPLER {
                                let &ShaderResourceDescriptorData::Image2D { image_view, sampler } =
                                        world_shader_resource_descriptors_data.get(resource_id).unwrap()
                                else {
                                        panic!();
                                };

                                let image_info = vk::DescriptorImageInfo {
                                        sampler,
                                        image_view,
                                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                };

                                let write = vk::WriteDescriptorSet::builder()
                                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                                        .dst_set(vk_shader.world_dst_set[framei])
                                        .dst_binding(binding.binding)
                                        .dst_array_element(0)
                                        .image_info(image_info.ref_into_slice())
                                        .build();

                                unsafe { device.update_descriptor_sets(&[write], &[]) };
                        }
                }
        }

        fn update_skybox(
                vk_asset_manager: &VkAssetManager,
                world_shader_resource_descriptors_data: &mut HashMap<ShaderResourceId, ShaderResourceDescriptorData>,
                skybox: &VkCubemap,
        ) {
                Self::write_image_resource(
                        vk_asset_manager,
                        &SHADER_RESOURCE_SKYBOX,
                        *skybox.image_view,
                        *skybox.sampler,
                        world_shader_resource_descriptors_data,
                );
        }
}

struct VkFrameData {
        // Signaled when a swapchain image has become available for presentation. vkAcquireImage may return an image that is not immediately available.
        img_available_semaphore: VkSemaphore,
        // Signaled when all the rendering commands for a frame have finished executing, which means that the rendered image is now ready for presentation.
        render_finished_semaphore: VkSemaphore,
        // Command buffer used for submitting draw operations of one frame.
        draw_cmd_buffer: VkReusableCommandBuffer,
}

impl VkFrameData {
        fn new(
                vk_context: &mut VkContext,
                // world_dst_set_layout: vk::DescriptorSetLayout,
                // object_dst_set_layout: vk::DescriptorSetLayout,
        ) -> AnyResult<Self> {
                let semaphore_cinfo = vk::SemaphoreCreateInfo::builder().build();
                let img_available_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };
                let render_finished_semaphore = unsafe { VkSemaphore::new(&vk_context.device, &semaphore_cinfo)? };

                let draw_cmd_buffer =
                        VkReusableCommandBuffer::new(Rc::clone(&vk_context.device), Rc::clone(&vk_context.cmd_pool))?;

                Ok(Self {
                        img_available_semaphore,
                        render_finished_semaphore,
                        draw_cmd_buffer,
                })
        }
}

impl Drop for VkFrameData {
        fn drop(&mut self) {
                unsafe {
                        self.draw_cmd_buffer.destroy();
                        self.render_finished_semaphore.destroy();
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
#[derive(ShaderStruct)]
// TODO: move out of this file (as it is not vulkan specific).
pub struct WorldMatrices {
        view_pos: Vec4,
        view: Mat4,
        proj: Mat4,
        vp: Mat4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
struct WorldDirectionalLight {
        vp: Mat4,
        direction: Vec4,
        color: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
struct WorldPointLight {
        vp_mats: [Mat4; 6],
        pos: Vec4,
        color: Vec4,
        kc_kl_kq: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
struct WorldSpotlight {
        pos: Vec4,
        dir: Vec4, // xyz=direction w=angle
        color: Vec4,
        kc_kl_kq_inner: Vec4, // w=inner radius percentage
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
pub struct WorldLights {
        dir_light: WorldDirectionalLight,
        point_light: WorldPointLight,
        spotlight: WorldSpotlight,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
pub struct MaterialData {
        pub ambient_color: Vec4,
        pub diffuse_color: Vec4,
        pub specular_color: Vec4,
        pub shininess_and_ambient_strength: Vec2,
        pub specular_strength_and_diffuse_strength: Vec2,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
pub struct BillboardData {
        billboard_center: Vec4,
        billboard_scale: Vec4,
        camera_right: Vec4,
        camera_up: Vec4,
}

#[allow(dead_code)]
#[repr(C)]
#[derive(ShaderStruct)]
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

enum ShaderResourceDescriptorData {
        Image2D {
                image_view: vk::ImageView,
                sampler: vk::Sampler,
        },
}

struct VkRenderScene {
        skybox_object_matrices_offset: Option<usize>,
        mesh_instances: SecondaryMap<ShaderId, SecondaryMap<MaterialId, Vec<VkMeshInstance>>>,
}

struct VkMeshInstance {
        mesh: MeshId,
        object_matrices_dynamic_offset: usize,
}
