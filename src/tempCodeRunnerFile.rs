self.input_manager.on_device_event(&event, |action_event| {
					self.logic_thread_tx.send(LogicThreadMessage::ActionEvent(action_event));
				});