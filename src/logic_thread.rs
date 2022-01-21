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
        input_manager::ActionReceiver,
        my_glm::{Mat4, Quat, UnitQuat, Vec3},
        render_state_switcher::RenderStateSwitcher,
        renderer::{ModelInstance, RenderState},
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

                // world.insert(ActionEventChannel::new());
                world.insert(QueuedWindowThreadMessagesResource::default());

                let mut dispatcher = DispatcherBuilder::new()
                        .with(
                                InputHandlerSystem {
                                        // reader_id: None,
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
                                RelativePositionUpdaterSystem::default(),
                                "relative-position-updater",
                                &[],
                        )
                        .with(
                                RelativeOrientationUpdaterSystem::default(),
                                "relative-orientation-updater",
                                &[],
                        )
                        .with(
                                PendingMovementResolverSystem,
                                "pending-movement-resolver-system",
                                &["relative-position-updater"],
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
                        .with(PositionComponent(Vec3::new(0.0, 0.0, 2.0)))
                        //.with(OrientationComponent(UnitQuat::from_axis_angle(&Vec3::x_axis(), 0f32.to_radians(),)))
                        .with(OrientationComponent(UnitQuat::from_axis_angle(
                                &Vec3::x_axis(),
                                0.0f32.to_radians(),
                        )))
                        .build();
                world.insert(PlayerResource(player));

                let camera = world
                        .create_entity()
                        .with(ParentComponent(player))
                        .with(PositionComponent::default())
                        .with(RelativePositionComponent(Vec3::new(0.0, 1.0, 0.0)))
                        .with(OrientationComponent::default())
                        .with(RelativeOrientationComponent::default())
                        .with(ProjectionCameraComponent::new(90.0f32.to_radians(), 1.0, 0.1, 100.0))
                        .build();
                world.insert(ActiveCameraResource(camera));

                let _colt = world
                        .create_entity()
                        .with(PositionComponent(Vec3::new(1.0, 1.0, 0.0)))
                        .with(OrientationComponent(UnitQuat::identity()))
                        .with(ModelComponent(params.asset_manager.get_model_by_name("colt")))
                        .build();

                'main: loop {
                        let begin = Instant::now();

                        for message in params.logic_thread_rx.try_iter() {
                                match message {
                                        LogicThreadMessage::Command(command) => match command {
                                                LogicThreadCommand::Exit => break 'main,
                                        },
                                        // LogicThreadMessage::ActionEvent(action_event) => {
                                        //         world.fetch_mut::<ActionEventChannel>().single_write(action_event);
                                        // },
                                        LogicThreadMessage::SetPlayerOrien(new_player_orien) => {
                                                world.write_storage::<OrientationComponent>()
                                                        .get_mut(player)
                                                        .unwrap()
                                                        .0 = new_player_orien;
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
        // ActionEvent(ActionEvent),
        SetPlayerOrien(UnitQuat),
}
pub enum LogicThreadCommand {
        Exit,
}

struct InputHandlerSystem {
        // reader_id: Option<ReaderId<ActionEvent>>,
        action_receiver: ActionReceiver,
        // continuous_actions: Arc<Mutex<ActionPollableState>>,
        last_cursor_state: CursorState,
        last_window_mode: WindowMode,
}

impl<'a> specs::System<'a> for InputHandlerSystem {
        type SystemData = (
                ReadExpect<'a, TargetTicktimeF32Resource>,
                ReadExpect<'a, PlayerResource>,
                WriteExpect<'a, QueuedWindowThreadMessagesResource>,
                WriteStorage<'a, OrientationComponent>,
                WriteStorage<'a, PositionComponent>,
        );

        fn run(
                &mut self,
                (target_ticktime, player, mut queued_window_thread_messages, mut orien_strg, mut pos_strg): Self::SystemData,
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
                        let player_orien = &orien_strg.get(player.0).unwrap().0;
                        let player_hor_orien = UnitQuat::new_normalize(Quat::new(
                                player_orien.as_vector().w,
                                0.0,
                                player_orien.as_vector().y,
                                0.0,
                        ));

                        let move_amount = PLAYER_MOVEMENT_SPEED * target_ticktime.0;
                        let move_dir = player_hor_orien * desired_dir.normalize() * move_amount;

                        let player_pos = &mut pos_strg.get_mut(player.0).unwrap().0;
                        *player_pos += move_dir;
                }
        }

        fn setup(&mut self, world: &mut World) {
                Self::SystemData::setup(world);

                // self.reader_id = Some(WriteExpect::<ActionEventChannel>::fetch(world).register_reader());
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

#[derive(Debug, Default)]
struct PositionComponent(Vec3);

impl Component for PositionComponent {
        type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

#[derive(Debug)]
struct PendingMovementComponent(Vec3);

impl Component for PendingMovementComponent {
        type Storage = VecStorage<Self>;
}

#[derive(Debug, Default)]
struct RelativePositionComponent(Vec3);

impl Component for RelativePositionComponent {
        type Storage = DenseVecStorage<Self>;
}

#[derive(Debug, Default)]
struct RelativeOrientationComponent(UnitQuat);

impl Component for RelativeOrientationComponent {
        type Storage = VecStorage<Self>;
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

#[derive(Debug, Clone, Default)]
struct OrientationComponent(UnitQuat);

impl Component for OrientationComponent {
        type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

#[derive(Debug, Component)]
#[storage(DenseVecStorage)]
struct ModelComponent(ModelId);

#[derive(Default)]
struct RelativePositionUpdaterSystem {
        reader_id: Option<ReaderId<ComponentEvent>>,
}

impl<'a> specs::System<'a> for RelativePositionUpdaterSystem {
        type SystemData = (
                Entities<'a>,
                ReadStorage<'a, ParentComponent>,
                ReadStorage<'a, PositionComponent>,
                ReadStorage<'a, RelativePositionComponent>,
                WriteStorage<'a, PendingMovementComponent>,
        );

        fn run(&mut self, (entities, parent_strg, pos_strg, rel_pos_strg, mut pending_mov_strg): Self::SystemData) {
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

                for (e, parent, rel_pos) in (&entities, &parent_strg, &rel_pos_strg).join() {
                        let parent_pos = match pos_strg.get(parent.0) {
                                Some(parent_pos) => parent_pos,
                                None => continue,
                        };

                        pending_mov_strg
                                .insert(e, PendingMovementComponent(parent_pos.0 + rel_pos.0))
                                .unwrap();
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

                self.reader_id = Some(WriteStorage::<PositionComponent>::fetch(world).register_reader());
        }
}

#[derive(Default)]
struct RelativeOrientationUpdaterSystem {
        reader_id: Option<ReaderId<ComponentEvent>>,
}

impl<'a> specs::System<'a> for RelativeOrientationUpdaterSystem {
        type SystemData = (
                Entities<'a>,
                ReadStorage<'a, ParentComponent>,
                ReadStorage<'a, RelativeOrientationComponent>,
                WriteStorage<'a, OrientationComponent>,
        );

        fn run(&mut self, (entities, parent_strg, rel_orien_strg, mut orien_strg): Self::SystemData) {
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

                for (e, parent, rel_orien) in (&entities, &parent_strg, &rel_orien_strg).join() {
                        let parent_orien = match orien_strg.get(parent.0) {
                                Some(parent_pos) => parent_pos.0,
                                None => continue,
                        };

                        orien_strg.get_mut(e).unwrap().0 = parent_orien * rel_orien.0;
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

                self.reader_id = Some(WriteStorage::<OrientationComponent>::fetch(world).register_reader());
        }
}

struct ModelRotationSystem;

impl<'a> specs::System<'a> for ModelRotationSystem {
        type SystemData = (
                ReadExpect<'a, TargetTicktimeF32Resource>,
                ReadStorage<'a, ModelComponent>,
                WriteStorage<'a, PositionComponent>,
                WriteStorage<'a, OrientationComponent>,
        );

        fn run(&mut self, (ticktime, mdl_strg, mut pos_strg, mut orien_strg): Self::SystemData) {
                for (_, pos, orien) in (&mdl_strg, &mut pos_strg, &mut orien_strg).join() {
                        let mov = UnitQuat::from_axis_angle(&Vec3::y_axis(), -22.5f32.to_radians() * ticktime.0);
                        pos.0 = mov * pos.0;

                        let rot = UnitQuat::from_axis_angle(&Vec3::y_axis(), 45.0f32.to_radians() * ticktime.0);
                        orien.0 = rot * orien.0;
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
                WriteStorage<'a, PositionComponent>,
        );

        fn run(&mut self, (mut pending_mov_strg, mut pos_strg): Self::SystemData) {
                for (pending_mov, pos) in (pending_mov_strg.drain(), &mut pos_strg).join() {
                        pos.0 = pending_mov.0;
                }
        }
}

struct QuaternionRenormalizationSystem;

impl<'a> specs::System<'a> for QuaternionRenormalizationSystem {
        type SystemData = WriteStorage<'a, OrientationComponent>;

        fn run(&mut self, mut orien_strg: Self::SystemData) {
                for orien in (&mut orien_strg).join() {
                        orien.0 = UnitQuat::new_unchecked(orien.0.normalize());
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
                ReadStorage<'a, ProjectionCameraComponent>,
                ReadStorage<'a, PositionComponent>,
                ReadStorage<'a, ModelComponent>,
                ReadStorage<'a, OrientationComponent>,
        );

        fn run(&mut self, (active_cam, proj_cams, pos_strg, mdl_strg, orien_strg): Self::SystemData) {
                let mut render_state = self.render_state.take().unwrap_or_else(|| Box::new(RenderState::new()));

                render_state.camera_pos = pos_strg.get(active_cam.0).unwrap().0;
                render_state.proj_camera = proj_cams.get(active_cam.0).unwrap().clone();

                render_state.model_instances.clear();
                for (pos, model, orien) in (&pos_strg, &mdl_strg, &orien_strg).join() {
                        render_state.model_instances.insert(
                                model.0,
                                ModelInstance {
                                        pos: pos.0,
                                        orien: orien.0,
                                },
                        );
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
