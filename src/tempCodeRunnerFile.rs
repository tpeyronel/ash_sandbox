let player_hor_orien = UnitQuat::new_normalize(Quat::new(
                                                player_orien.as_vector().w,
                                                0.0,
                                                player_orien.as_vector().y,
                                                0.0,
                                        ));
                                        let right_dir = player_hor_orien * Vec3::x_axis();
                                        info!("{:#?}", right_dir);