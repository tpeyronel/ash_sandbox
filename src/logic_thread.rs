use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
};

use specs::{
        prelude::ComponentEvent, storage::GenericWriteStorage, BitSet, Builder, Component, DenseVecStorage,
        DispatcherBuilder, Entities, Entity, FlaggedStorage, Join, ReadExpect, ReadStorage, ReaderId, SystemData,
        VecStorage, World, WorldExt, WriteExpect, WriteStorage,
};

#[allow(unused_imports)]
use log::{error, info, trace, warn};

use crate::{
        actions::*,
        application::{CursorState, WindowMode, WindowThreadCommand, WindowThreadMessage},
        asset_manager::{AssetManager, ModelId},
        constants::PLAYER_MOVEMENT_SPEED,
        hashmap::GetOrInsert,
        input_manager::ActionReceiver,
        my_glm::{Mat4, Quat, UnitQuat, Vec3},
        render_state_switcher::RenderStateSwitcher,
        renderer::{LightColor, LightPos, ModelInstance, ModelInstanceId, RenderState},
};

#[derive(Debug, Clone, Copy)]
struct TargetTpsResource(u32);

#[derive(Debug, Clone, Copy)]
struct TargetTicktimeResource(Duration);

#[derive(Debug, Clone, Copy)]
struct TargetTicktimeF32Resource(f32);

#[derive(Debug, Clone, Copy)]
struct ActiveCameraResource(Entity);

#[derive(Debug, Clone, Copy)]
struct PlayerResource(Entity);

#[derive(Default)]
struct QueuedWindowThreadMessagesResource(VecDeque<WindowThreadMessage>);

pub struct LogicThreadSpawnParams {
        pub target_tps: u32,
        pub logic_thread_rx: std::sync::mpsc::Receiver<LogicThreadMessage>,
        pub window_thread_tx: std::sync::mpsc::Sender<WindowThreadMessage>,
        pub action_receiver: ActionReceiver,
        pub asset_manager: Arc<AssetManager>,
        pub render_state_switcher: Arc<Mutex<RenderStateSwitcher>>,
}

pub struct LogicThread {
        handle: std::thread::JoinHandle<()>,
}

impl LogicThread {
        pub fn spawn(params: LogicThreadSpawnParams) -> Self {
                Self {
                        handle: std::thread::Builder::new()
                                .name("Logic".to_string())
                                .spawn(|| Self::run(params))
                                .expect("Failed to spawn logic thread!"),
                }
        }

        pub fn join(self) -> std::thread::Result<()> {
                self.handle.join()
        }

        fn run(params: LogicThreadSpawnParams) {
                let mut world = World::new();

                let target_ticktime = Duration::from_secs_f64(1.0 / params.target_tps as f64);
                world.insert(TargetTpsResource(params.target_tps));
                world.insert(TargetTicktimeResource(target_ticktime));
                world.insert(TargetTicktimeF32Resource(target_ticktime.as_secs_f32()));

                world.insert(QueuedWindowThreadMessagesResource::default());

                let mut dispatcher = DispatcherBuilder::new()
                        .with(
                                InputHandlerSystem {
                                        action_receiver: params.action_receiver,
                                        last_cursor_state: CursorState::Normal,
                                        last_window_mode: WindowMode::Windowed,
                                },
                                "input-handler-system",
                                &[],
                        )
                        .with(
                                FamilyHierarchySynchronizerSystem::default(),
                                "family-hierarchy-synchronizer",
                                &[],
                        )
                        .with(
                                WindowThreadMessageDispatcherSystem {
                                        window_thread_tx: params.window_thread_tx,
                                },
                                "window-thread-message-dispatcher",
                                &[],
                        )
                        .with(
                                RelativeTransformUpdaterSystem::default(),
                                "relative-transform-updater",
                                &[],
                        )
                        .with(
                                PendingMovementResolverSystem,
                                "pending-movement-resolver-system",
                                &["relative-transform-updater"],
                        )
                        .with(
                                ModelRotationSystem,
                                "model-rotation",
                                &["pending-movement-resolver-system"],
                        )
                        .with(
                                QuaternionRenormalizationSystem,
                                "quaternion-renormalization-system",
                                &[],
                        )
                        .with_thread_local(RenderStateGeneratorSystem {
                                render_state: None,
                                render_state_switcher: params.render_state_switcher,
                        })
                        .build();
                dispatcher.setup(&mut world);

                let player = world
                        .create_entity()
                        .with(TransformComponent::from_pos(Vec3::new(0.0, 0.0, 2.0)))
                        .build();
                world.insert(PlayerResource(player));

                let camera = world
                        .create_entity()
                        .with(TransformComponent::default())
                        .with(ParentComponent(player))
                        .with(RelativeTransformComponent(TransformComponent::from_pos(Vec3::new(0.0, 1.0, 0.0))))
                        .with(ProjectionCameraComponent::new(90.0f32.to_radians(), 1.0, 0.1, 100.0))
                        .build();
                world.insert(ActiveCameraResource(camera));

                let _colt = world
                        .create_entity()
                        .with(TransformComponent::from_pos(Vec3::new(2.5, 0.0, 0.0)))
                        .with(ModelComponent(params.asset_manager.get_model_by_name("colt")))
                        .with(ModelRotateComponent(-22.5f32.to_radians()))
                        .build();

                let _icosphere = world
                        .create_entity()
                        .with(TransformComponent::from_scale(Vec3::new(4.0, 4.0, 4.0)))
                        .with(ModelComponent(params.asset_manager.get_model_by_name("icosphere")))
                        .with(ModelRotateComponent(0f32.to_radians()))
                        .build();

                let _light = world
                        .create_entity()
                        .with(TransformComponent::from_pos(Vec3::new(1.0, 2.0, 0.0)))
                        .with(ModelComponent(params.asset_manager.get_model_by_name("icosphere")))
                        .with(LightEmitterComponent {
                                color: Vec3::new(1.0, 0.8, 0.8),
                        })
                        .with(ModelRotateComponent(45f32.to_radians()))
                        .build();

                'main: loop {
                        let begin = Instant::now();

                        for message in params.logic_thread_rx.try_iter() {
                                match message {
                                        LogicThreadMessage::Command(command) => match command {
                                                LogicThreadCommand::Exit => break 'main,
                                        },
                                        LogicThreadMessage::SetPlayerOrien(new_player_orien) => {
                                                world.write_storage::<TransformComponent>()
                                                        .get_mut(player)
                                                        .unwrap()
                                                        .orien = new_player_orien;
                                        },
                                }
                        }

                        dispatcher.dispatch(&mut world);
                        world.maintain();

                        while (Instant::now() - begin) < target_ticktime {}
                }
        }
}

pub enum LogicThreadMessage {
        Command(LogicThreadCommand),
        SetPlayerOrien(UnitQuat),
}

pub enum LogicThreadCommand {
        Exit,
}

struct InputHandlerSystem {
        action_receiver: ActionReceiver,
        last_cursor_state: CursorState,
        last_window_mode: WindowMode,
}

impl<'a> specs::System<'a> for InputHandlerSystem {
        type SystemData = (
                ReadExpect<'a, TargetTicktimeF32Resource>,
                ReadExpect<'a, PlayerResource>,
                WriteExpect<'a, QueuedWindowThreadMessagesResource>,
                WriteStorage<'a, TransformComponent>,
        );

        fn run(
                &mut self,
                (target_ticktime, player, mut queued_window_thread_messages, mut transforms): Self::SystemData,
        ) {
                let mut desired_dir = Vec3::new(0.0, 0.0, 0.0);

                for (action_id, strength) in self.action_receiver.receive() {
                        match action_id {
                                MOVE_FORWARD => desired_dir.z -= strength.0,
                                MOVE_BACKWARD => desired_dir.z += strength.0,
                                MOVE_RIGHTWARD => desired_dir.x += strength.0,
                                MOVE_LEFTWARD => desired_dir.x -= strength.0,
                                MOVE_UPWARD => desired_dir.y += strength.0,
                                MOVE_DOWNARD => desired_dir.y -= strength.0,
                                EXIT => {
                                        let command = WindowThreadMessage::Command(WindowThreadCommand::Exit);
                                        queued_window_thread_messages.0.push_back(command);
                                },
                                TOGGLE_CURSOR => {
                                        self.last_cursor_state = match self.last_cursor_state {
                                                CursorState::Normal => CursorState::Hidden,
                                                CursorState::Hidden => CursorState::Normal,
                                        };

                                        queued_window_thread_messages.0.push_back(WindowThreadMessage::Command(
                                                WindowThreadCommand::SetCursorState(self.last_cursor_state),
                                        ));

                                        queued_window_thread_messages.0.push_back(WindowThreadMessage::Command(
                                                WindowThreadCommand::SetPlayerCameraEnabled(
                                                        self.last_cursor_state == CursorState::Hidden,
                                                ),
                                        ));
                                },
                                CYCLE_WINDOW_MODE => {
                                        self.last_window_mode = match self.last_window_mode {
                                                WindowMode::Windowed => WindowMode::Borderless,
                                                WindowMode::Borderless => WindowMode::Fullscreen,
                                                WindowMode::Fullscreen => WindowMode::Windowed,
                                        };

                                        queued_window_thread_messages.0.push_back(WindowThreadMessage::Command(
                                                WindowThreadCommand::SetWindowMode(self.last_window_mode),
                                        ));
                                },
                                _ => (),
                        }
                }

                if desired_dir.norm_squared() > f32::EPSILON {
                        let player_orien = &transforms.get(player.0).unwrap().orien;
                        let player_hor_orien = UnitQuat::new_normalize(Quat::new(
                                player_orien.as_vector().w,
                                0.0,
                                player_orien.as_vector().y,
                                0.0,
                        ));

                        let move_amount = PLAYER_MOVEMENT_SPEED * target_ticktime.0;
                        let move_dir = player_hor_orien * desired_dir.normalize() * move_amount;

                        let player_pos = &mut transforms.get_mut(player.0).unwrap().pos;
                        *player_pos += move_dir;
                }
        }
}

#[derive(Default)]
struct FamilyHierarchySynchronizerSystem {
        reader_id: Option<ReaderId<ComponentEvent>>,
}

impl<'a> specs::System<'a> for FamilyHierarchySynchronizerSystem {
        type SystemData = (ReadStorage<'a, ParentComponent>, WriteStorage<'a, ChildrenComponent>);

        fn run(&mut self, (parent_strg, mut children_strg): Self::SystemData) {
                let mut new_children = BitSet::new();

                let events = parent_strg.channel().read(self.reader_id.as_mut().unwrap());
                for event in events {
                        match event {
                                ComponentEvent::Inserted(id) => {
                                        new_children.add(*id);
                                },
                                _ => (),
                        }
                }

                for (parent, new_child) in (&parent_strg, &new_children).join() {
                        let parent_children = match children_strg.get_mut_or_default(parent.0) {
                                Some(parent) => parent,
                                None => continue,
                        };

                        parent_children.0.add(new_child);
                }
        }

        fn setup(&mut self, world: &mut World) {
                Self::SystemData::setup(world);

                self.reader_id = Some(WriteStorage::<ParentComponent>::fetch(world).register_reader());
        }
}

struct WindowThreadMessageDispatcherSystem {
        window_thread_tx: std::sync::mpsc::Sender<WindowThreadMessage>,
}

impl<'a> specs::System<'a> for WindowThreadMessageDispatcherSystem {
        type SystemData = WriteExpect<'a, QueuedWindowThreadMessagesResource>;

        fn run(&mut self, mut queued_messages: Self::SystemData) {
                while let Some(message) = queued_messages.0.pop_front() {
                        if let Err(err) = self.window_thread_tx.send(message) {
                                error!("Error ocurred sending message to window thread: {}", err);
                        }
                }
        }
}

#[derive(Debug)]
struct TransformComponent {
        pub pos: Vec3,
        pub orien: UnitQuat,
        pub scale: Vec3,
}

impl Component for TransformComponent {
        type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

impl Default for TransformComponent {
        fn default() -> Self {
                Self {
                        pos: Vec3::from_element(0.0),
                        orien: UnitQuat::identity(),
                        scale: Vec3::from_element(1.0),
                }
        }
}

impl TransformComponent {
        pub fn from_pos(pos: Vec3) -> Self {
                Self {
                        pos,
                        ..Default::default()
                }
        }

        pub fn from_orien(orien: UnitQuat) -> Self {
                Self {
                        orien,
                        ..Default::default()
                }
        }

        pub fn from_scale(scale: Vec3) -> Self {
                Self {
                        scale,
                        ..Default::default()
                }
        }
}

#[derive(Debug)]
struct PendingMovementComponent(Vec3);

impl Component for PendingMovementComponent {
        type Storage = VecStorage<Self>;
}

#[derive(Debug, Default)]
struct RelativeTransformComponent(TransformComponent);

impl Component for RelativeTransformComponent {
        type Storage = DenseVecStorage<Self>;
}

#[derive(Debug)]
struct ParentComponent(Entity);

impl Component for ParentComponent {
        type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

#[derive(Debug, Clone, Default)]
struct ChildrenComponent(BitSet);

impl Component for ChildrenComponent {
        type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

#[derive(Debug, Component)]
#[storage(DenseVecStorage)]
struct ModelComponent(ModelId);

#[derive(Debug, Component)]
#[storage(VecStorage)]
struct ModelRotateComponent(f32);

#[derive(Debug, Component)]
#[storage(VecStorage)]
struct LightEmitterComponent {
        color: Vec3,
}

#[derive(Default)]
struct RelativeTransformUpdaterSystem {
        reader_id: Option<ReaderId<ComponentEvent>>,
}

impl<'a> specs::System<'a> for RelativeTransformUpdaterSystem {
        type SystemData = (
                Entities<'a>,
                ReadStorage<'a, ParentComponent>,
                WriteStorage<'a, TransformComponent>,
                ReadStorage<'a, RelativeTransformComponent>,
                WriteStorage<'a, PendingMovementComponent>,
        );

        fn run(&mut self, (entities, parent_strg, mut transforms, rel_transforms, mut pending_mov_strg): Self::SystemData) {
                /* let moved_entities = BitSet::new();

                let events = pos_strg.channel().read(self.reader_id.as_mut().unwrap());
                for event in events {
                        match event {
                                ComponentEvent::Modified(id) => {
                                        moved_entities.add(*id);
                                }
                                _ => (),
                        }
                }

                for (child_rel_pos, child_pos, _) in (&rel_pos_strg, &mut pos_strg, &moved_entities).join() {
                        let parent_pos = pos_strg.get(e)

                        child_pos.0 = child_rel_pos.0 +
                } */

                for (e, parent, rel_transform) in (&entities, &parent_strg, &rel_transforms).join() {
                        let parent_transform = match transforms.get(parent.0) {
                                Some(parent_transform) => parent_transform,
                                None => continue,
                        };

                        pending_mov_strg
                                .insert(e, PendingMovementComponent(parent_transform.pos + rel_transform.0.pos))
                                .unwrap();

                        transforms.get_mut(e).unwrap().orien = parent_transform.orien * rel_transform.0.orien;
                }

                /* let parent_positions: Vec<Vec3> = (&parent_strg, &pos_strg)
                        .join()
                        .map(|(_, parent_pos)| parent_pos.0)
                        .collect();

                for ((rel_pos, pos), parent_pos) in (&rel_pos_strg, &mut pos_strg).join().zip(parent_positions.iter()) {
                        pos.0 = parent_pos + rel_pos.value;
                } */
        }

        fn setup(&mut self, world: &mut World) {
                Self::SystemData::setup(world);

                self.reader_id = Some(WriteStorage::<TransformComponent>::fetch(world).register_reader());
        }
}

struct ModelRotationSystem;

impl<'a> specs::System<'a> for ModelRotationSystem {
        type SystemData = (
                ReadExpect<'a, TargetTicktimeF32Resource>,
                ReadStorage<'a, ModelComponent>,
                ReadStorage<'a, ModelRotateComponent>,
                WriteStorage<'a, TransformComponent>,
        );

        fn run(&mut self, (ticktime, mdl_strg, mdl_rotate_strg, mut transforms): Self::SystemData) {
                for (_, rotate, transform) in (&mdl_strg, &mdl_rotate_strg, &mut transforms).join() {
                        let mov = UnitQuat::from_axis_angle(&Vec3::y_axis(), -rotate.0 * ticktime.0);
                        transform.pos = mov * transform.pos;

                        let rot = UnitQuat::from_axis_angle(&Vec3::y_axis(), rotate.0 * 2.0 * ticktime.0);
                        transform.orien = rot * transform.orien;
                }
        }
}

#[derive(Debug, Default, Clone, Component)]
#[storage(VecStorage)]
pub struct ProjectionCameraComponent {
        fovy: f32,
        zoom: f32,
        near: f32,
        far: f32,
}

impl ProjectionCameraComponent {
        fn new(fovy: f32, zoom: f32, near: f32, far: f32) -> Self {
                Self { fovy, zoom, near, far }
        }

        pub fn calc_proj_matrix(&self, aspect_ratio: f32) -> Mat4 {
                glm::perspective_rh_zo(aspect_ratio, self.fovy / self.zoom, self.near, self.far)
        }
}

struct PendingMovementResolverSystem;

impl<'a> specs::System<'a> for PendingMovementResolverSystem {
        type SystemData = (
                WriteStorage<'a, PendingMovementComponent>,
                WriteStorage<'a, TransformComponent>,
        );

        fn run(&mut self, (mut pending_mov_strg, mut transforms): Self::SystemData) {
                for (pending_mov, transform) in (pending_mov_strg.drain(), &mut transforms).join() {
                        transform.pos = pending_mov.0;
                }
        }
}

struct QuaternionRenormalizationSystem;

impl<'a> specs::System<'a> for QuaternionRenormalizationSystem {
        type SystemData = WriteStorage<'a, TransformComponent>;

        fn run(&mut self, mut transforms: Self::SystemData) {
                for transform in (&mut transforms).join() {
                        transform.orien = UnitQuat::new_unchecked(transform.orien.normalize());
                }
        }
}

struct RenderStateGeneratorSystem {
        render_state: Option<Box<RenderState>>,
        render_state_switcher: Arc<Mutex<RenderStateSwitcher>>,
}

impl<'a> specs::System<'a> for RenderStateGeneratorSystem {
        type SystemData = (
                ReadExpect<'a, ActiveCameraResource>,
                Entities<'a>,
                ReadStorage<'a, ProjectionCameraComponent>,
                ReadStorage<'a, ModelComponent>,
                ReadStorage<'a, TransformComponent>,
                ReadStorage<'a, LightEmitterComponent>,
        );

        fn run(&mut self, (active_cam, entities, proj_cams, mdl_strg, transforms, light_strg): Self::SystemData) {
                let mut render_state = self.render_state.take().unwrap_or_else(|| Box::new(RenderState::new()));

                render_state.camera_pos = transforms.get(active_cam.0).unwrap().pos;
                render_state.proj_camera = proj_cams.get(active_cam.0).unwrap().clone();

                render_state.model_instances.clear();
                for (e, model, transform) in (&entities, &mdl_strg, &transforms).join() {
                        render_state.model_instances.insert(
                                ModelInstanceId(e),
                                ModelInstance {
                                        model_id: model.0,
                                        pos: transform.pos,
                                        orien: transform.orien,
                                        scale: transform.scale,
                                },
                        );
                }

                render_state.lights.clear();
                for (e, transform, light) in (&entities, &transforms, &light_strg).join() {
                        render_state
                                .lights
                                .insert(e, (LightPos(transform.pos), LightColor(light.color)));
                }

                let mut render_state_switcher = match self.render_state_switcher.lock() {
                        Ok(lock) => lock,
                        Err(err) => {
                                error!("RENDER STATE SWITCHER MUTEX IS POISONED\n{}", err);
                                return;
                        },
                };
                self.render_state = render_state_switcher.write_render_state(render_state);
        }
}
