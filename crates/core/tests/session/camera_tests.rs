//! Cameras and where 3D views were left (`commands::camera`).

mod tests {
    use serde_json::json;

    use crate::engine::Session;
    use crate::model::{Element, ViewCamera};
    use crate::view3d::Camera;

    fn placed() -> Session {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 1600.0], "target": [0.0, 0.0, 1600.0]})).unwrap();
        s
    }

    fn read(s: &mut Session) -> serde_json::Value {
        s.execute("camera", &json!({"view": "Camera 1"})).unwrap()
    }

    fn cam_of(s: &Session) -> ViewCamera {
        s.model
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                Element::View3D(v) if v.name == "Camera 1" => v.camera,
                _ => None,
            })
            .unwrap()
    }

    /// The lens as 3ds Max's Physical Camera has it: a focal length, or a field of view across the film, each setting the
    /// other; a smaller film or a zoom narrows what a lens sees.
    #[test]
    fn a_camera_s_focal_length_and_field_of_view_set_each_other_across_its_film() {
        let mut s = placed();
        let r = read(&mut s);
        assert_eq!(r["lens"].as_f64(), Some(35.0));
        let fov = r["fov"].as_f64().unwrap();
        assert!((fov - 2.0 * (18.0f64 / 35.0).atan().to_degrees()).abs() < 1e-9, "{fov}");
        // 90 degrees across 36 mm of film is an 18 mm lens.
        let r = s.execute("camera", &json!({"view": "Camera 1", "fov": 90})).unwrap();
        assert!((r["lens"].as_f64().unwrap() - 18.0).abs() < 1e-3, "{r}");
        assert!((r["fov"].as_f64().unwrap() - 90.0).abs() < 1e-2);
        // A Four Thirds sensor behind the same lens sees less, and a zoom less again.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film_preset": "Four Thirds"})).unwrap();
        assert_eq!((r["film"].as_f64(), r["film_preset"].as_str()), (Some(17.3), Some("Four Thirds")));
        let narrow = r["fov"].as_f64().unwrap();
        assert!(narrow < 90.0);
        let r = s.execute("camera", &json!({"view": "Camera 1", "zoom": 2})).unwrap();
        assert!(r["fov"].as_f64().unwrap() < narrow);
        // A custom width reads as Custom.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film": 30})).unwrap();
        assert_eq!(r["film_preset"].as_str(), Some("Custom"));
        // The picture sees as wide as the field of view says: a point on its edge lands on the picture's edge.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film": 36, "zoom": 1, "fov": 90})).unwrap();
        assert_eq!(r["lens"].as_f64(), Some(18.0));
        let cam = Camera::through(&cam_of(&s), 18.0, true, 400, 300);
        let (x, ..) = cam.project([5000.0, -5000.0, 1600.0]);
        assert!((x - 400.0).abs() < 0.5, "{x}");
    }

    /// Revit's Eye Elevation and Target Elevation raise each point; Target Distance moves the target along the line of sight.
    #[test]
    fn eye_and_target_elevation_and_target_distance_move_the_camera_s_points() {
        let mut s = placed();
        let r = s.execute("camera", &json!({"view": "Camera 1", "eye_elevation": 1700, "target_elevation": 4000})).unwrap();
        assert_eq!((r["eye"].clone(), r["target"].clone()), (json!([0.0, -10000.0, 1700.0]), json!([0.0, 0.0, 4000.0])));
        let r = s
            .execute("camera", &json!({"view": "Camera 1", "eye_elevation": 1600, "target_elevation": 1600, "target_distance": 5000}))
            .unwrap();
        assert_eq!(r["target"], json!([0.0, -5000.0, 1600.0]));
        assert_eq!(r["target_distance"].as_f64(), Some(5000.0));
        // Focus follows the target until a distance of its own is given.
        assert_eq!((r["focus"].as_str(), r["focus_distance"].as_f64()), (Some("target"), Some(5000.0)));
        let r = s.execute("camera", &json!({"view": "Camera 1", "focus": 3000, "depth_of_field": true, "f_number": 2.8})).unwrap();
        assert_eq!(
            (r["focus_distance"].as_f64(), r["depth_of_field"].as_bool(), r["f_number"].as_f64()),
            (Some(3000.0), Some(true), Some(2.8))
        );
        let r = s.execute("camera", &json!({"view": "Camera 1", "focus": "target"})).unwrap();
        assert_eq!(r["focus_distance"].as_f64(), Some(5000.0));
        // A Free camera, and an orthographic one, read back so.
        let r = s.execute("camera", &json!({"view": "Camera 1", "targeted": false, "projection": "orthographic"})).unwrap();
        assert_eq!((r["targeted"].as_bool(), r["projection"].as_str()), (Some(false), Some("orthographic")));
    }

    /// Exposure as 3ds Max's: Target by an exposure value, or Manual by ISO, shutter and aperture, whose EV is worked out.
    #[test]
    fn exposure_is_an_ev_or_worked_out_from_iso_shutter_and_aperture() {
        let mut s = placed();
        let r = read(&mut s);
        assert_eq!((r["exposure"].as_str(), r["ev"].as_f64()), (Some("target"), Some(6.0)));
        let r = s.execute("camera", &json!({"view": "Camera 1", "ev": 12.5})).unwrap();
        assert_eq!(r["ev"].as_f64(), Some(12.5));
        // f/8 at 1/125 s and ISO 100: log2(64 × 125).
        let r = s.execute("camera", &json!({"view": "Camera 1", "iso": 100, "f_number": 8, "shutter": 0.008})).unwrap();
        assert_eq!(r["exposure"].as_str(), Some("manual"));
        assert!((r["ev"].as_f64().unwrap() - (64.0f64 / 0.008).log2()).abs() < 1e-9);
        let r = s.execute("camera", &json!({"view": "Camera 1", "exposure": "target"})).unwrap();
        assert_eq!((r["exposure"].as_str(), r["ev"].as_f64()), (Some("target"), Some(12.5)));
    }

    /// Output Size as Render Setup's: a size in px, a preset, and the aspect lock keeping the image's proportion when one side
    /// is given.
    #[test]
    fn output_size_takes_presets_and_keeps_its_aspect_when_locked() {
        let mut s = placed();
        assert_eq!(read(&mut s)["output"], json!([1920, 1080]));
        let r = s.execute("camera", &json!({"view": "Camera 1", "output_preset": "4K UHD"})).unwrap();
        assert_eq!((r["output"].clone(), r["output_preset"].as_str()), (json!([3840, 2160]), Some("4K UHD")));
        let r = s.execute("camera", &json!({"view": "Camera 1", "aspect_locked": true, "output": [1280, null]})).unwrap();
        assert_eq!(r["output"], json!([1280, 720]));
        let r = s.execute("camera", &json!({"view": "Camera 1", "output": [null, 1080]})).unwrap();
        assert_eq!(r["output"], json!([1920, 1080]));
        // Unlocked, both sides are needed; and the sides stay in range.
        s.execute("camera", &json!({"view": "Camera 1", "aspect_locked": false})).unwrap();
        for bad in [json!([1280, null]), json!([8, 8]), json!([100000, 1000]), json!("big")] {
            assert!(s.execute("camera", &json!({"view": "Camera 1", "output": bad})).is_err(), "{bad}");
        }
        // A render of the camera without a size is its output.
        s.execute("camera", &json!({"view": "Camera 1", "output": [320, 200]})).unwrap();
        let r = s.execute("render", &json!({"view": "Camera 1"})).unwrap();
        assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(320), Some(200)));
    }

    /// Perspective Control: with Auto Vertical Tilt Correction a camera looking up keeps verticals vertical and what it looks
    /// at in the middle; a lens shift moves the picture without turning the camera.
    #[test]
    fn auto_vertical_tilt_correction_keeps_verticals_vertical_and_lens_shift_moves_the_picture() {
        let mut s = placed();
        s.execute("camera", &json!({"view": "Camera 1", "target_elevation": 8000})).unwrap();
        let column = |cam: &Camera| {
            let (a, b) = (cam.project([3000.0, 0.0, 0.0]), cam.project([3000.0, 0.0, 9000.0]));
            (a.0 - b.0).abs()
        };
        let tilted = Camera::through(&cam_of(&s), 35.0, true, 400, 300);
        assert!(column(&tilted) > 5.0, "looking up, a column leans in: {}", column(&tilted));
        s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": true})).unwrap();
        let cam = cam_of(&s);
        let upright = Camera::through(&cam, 35.0, true, 400, 300);
        assert!(column(&upright) < 1e-6, "with the tilt corrected it stands upright: {}", column(&upright));
        let (x, y, _) = upright.project(cam.target);
        assert!((x - 200.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6, "the target stays in the middle: {x} {y}");
        let eye = upright.eye().unwrap();
        assert!((0..3).all(|k| (eye[k] - cam.eye[k]).abs() < 1e-6), "the eye stays: {eye:?}");
        assert_eq!(read(&mut s)["tilt"].as_f64(), Some(6400.0 / 10000.0), "Auto: the slope it looks up at");
        // A lens shift of 10% up moves the picture 10% of its width up the film: what shows moves down.
        s.execute("camera", &json!({"view": "Camera 1", "shift": [0, 10]})).unwrap();
        let shifted = Camera::through(&cam_of(&s), 35.0, true, 400, 300);
        let (_, y2, _) = shifted.project(cam.target);
        assert!((y2 - (150.0 + 40.0)).abs() < 1e-6, "{y2}");
        assert!(s.execute("camera", &json!({"view": "Camera 1", "shift": [0, 400]})).is_err());
        assert!(s.execute("camera", &json!({"view": "Camera 1", "tilt": [0, 2]})).is_err());
        assert!(s.execute("camera", &json!({"view": "Camera 1", "tilt": 90})).is_err(), "steeper than a camera turns");
    }

    /// Clipping as 3ds Max's Clip Manually, which is Revit's Far Clip too: what lies past the far plane is cut away from the
    /// picture.
    #[test]
    fn clipping_planes_cut_away_what_is_further() {
        let mut s = Session::new();
        s.execute("floor", &json!({"boundary": [[-3000.0, 3000.0], [3000.0, 3000.0], [3000.0, 9000.0], [-3000.0, 9000.0]]})).unwrap();
        s.execute("camera", &json!({"eye": [0.0, -5000.0, 1600.0], "target": [0.0, 6000.0, 0.0]})).unwrap();
        let picture =
            |s: &mut Session| s.execute("render", &json!({"view": "Camera 1", "width": 200, "height": 150})).unwrap()["pngBase64"].clone();
        let whole = picture(&mut s);
        s.execute("camera", &json!({"view": "Camera 1", "clip": true, "far": 20000})).unwrap();
        assert_eq!(picture(&mut s), whole, "the far plane past the floor cuts nothing");
        s.execute("camera", &json!({"view": "Camera 1", "far": 2000})).unwrap();
        assert_ne!(picture(&mut s), whole, "the far plane short of the floor cuts it away");
        let r = read(&mut s);
        assert_eq!((r["clip"].as_bool(), r["far"].as_f64()), (Some(true), Some(2000.0)));
        assert!(s.execute("camera", &json!({"view": "Camera 1", "near": 3000})).is_err(), "the near plane past the far one");
    }

    /// Strict params: a misspelled key is refused, and everything set is saved with the project.
    #[test]
    fn a_misspelled_setting_is_refused_and_the_settings_are_saved() {
        let mut s = placed();
        assert!(s.execute("camera", &json!({"view": "Camera 1", "focal": 50})).is_err());
        s.execute(
            "camera",
            &json!({"view": "Camera 1", "f_number": 4, "auto_tilt": true, "output": [1000, 1000], "clip": true, "far": 9000}),
        )
        .unwrap();
        let text = s.model.to_json().unwrap();
        let reopened = crate::model::Model::read(&text).unwrap().0;
        let again = reopened
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                Element::View3D(v) if v.name == "Camera 1" => v.camera,
                _ => None,
            })
            .unwrap();
        assert_eq!(again, cam_of(&s));
        assert_eq!((again.physical.f_number, again.physical.auto_tilt, again.physical.far), (4.0, true, 9000.0));
    }
}

mod review_tests {
    use serde_json::json;

    use crate::engine::Session;
    use crate::model::{Element, View3D};
    use crate::view3d::Camera;

    fn view(s: &Session, name: &str) -> View3D {
        s.model
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                Element::View3D(v) if v.name == name => Some(v.clone()),
                _ => None,
            })
            .unwrap()
    }

    /// The limits are on the lens as a 35 mm camera's (the field of view), whatever the film: a small sensor goes as wide as
    /// a full frame does, and a long lens on it is refused where it would be past 600 mm on 35 mm film, with a message
    /// saying so, never clamped in one place and not in another.
    #[test]
    fn a_lens_s_limits_are_its_35_mm_equivalent_s() {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 1600.0], "target": [0.0, 0.0, 1600.0], "film_preset": "Four Thirds"})).unwrap();
        // 8 mm on Four Thirds sees as 16.6 mm does on 35 mm film: wide, and allowed.
        let r = s.execute("camera", &json!({"view": "Camera 1", "lens": 8})).unwrap();
        assert_eq!(r["lens"].as_f64(), Some(8.0), "{r}");
        // 300 mm there is 624 mm on 35 mm film: refused, saying what it is.
        let e = s.execute("camera", &json!({"view": "Camera 1", "lens": 300})).unwrap_err();
        assert!(e.contains("35 mm") && e.contains("600"), "{e}");
        // The same for a field of view out of the range, and for a zoom that would take it past.
        assert!(s.execute("camera", &json!({"view": "Camera 1", "fov": 150})).is_err());
        s.execute("camera", &json!({"view": "Camera 1", "lens": 250})).unwrap();
        assert!(s.execute("camera", &json!({"view": "Camera 1", "zoom": 2})).is_err());
        // What is allowed draws as the cone says: render's field of view is the camera's.
        s.execute("camera", &json!({"view": "Camera 1", "film_preset": "35mm (Full Frame)", "lens": 600, "zoom": 1})).unwrap();
        let v = view(&s, "Camera 1");
        let cam = Camera::through(&v.camera.unwrap(), 600.0, true, 400, 300);
        let fov = crate::camera::field_of_view(600.0, &v.camera.unwrap().physical).to_radians();
        let (x, ..) = cam.project([10000.0 * (fov / 2.0).tan(), 0.0, 1600.0]);
        assert!((x - 400.0).abs() < 0.5, "{x}");
    }

    /// Tilt Correction is 3ds Max's: the film plane tilted by that slope, as `IPhysicalCamera::GetTiltCorrection` applies
    /// it (the ray's target on the focus plane moved back by y × tilt, its height divided by √(1 + tilt²)). Auto Vertical
    /// Tilt Correction sets it to what aligns the perspective with the Z axis: the slope the camera looks up at.
    #[test]
    fn tilt_correction_is_3ds_max_s_slope_and_auto_sets_it_to_the_camera_s() {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 1600.0], "target": [0.0, 0.0, 6600.0]})).unwrap();
        let r = s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": true})).unwrap();
        assert!((r["tilt"].as_f64().unwrap() - 0.5).abs() < 1e-9, "looking up 5000 in 10000 is a slope of 0.5: {r}");
        s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": false, "tilt": 0.3})).unwrap();
        let v = view(&s, "Camera 1");
        let vc = v.camera.unwrap();
        let (w, h) = (400u32, 300u32);
        let seen = Camera::through(&vc, 35.0, true, w, h);
        // 3ds Max's ray, by hand: the untilted camera's axes, its focal length in px.
        let bare = Camera::bare(&vc, 35.0, true, w, h);
        let (back, right, up) = bare.axes();
        let f = f64::from(w) * 35.0 / crate::camera::FILM;
        let t: f64 = 0.3;
        for p in [[3000.0, 0.0, 0.0], [-2000.0, 4000.0, 9000.0], [500.0, -3000.0, 2000.0]] {
            let d: [f64; 3] = std::array::from_fn(|k| p[k] - vc.eye[k]);
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let (wx, wy, wz) = (dot(d, right), dot(d, up), dot(d, back));
            // W = λ (x, Y, -f - Y t): λ from the depth, then the film's height y = Y √(1 + t²).
            let lambda = -(wz + wy * t) / f;
            let (x, y) = (wx / lambda, wy / lambda * (1.0 + t * t).sqrt());
            let (px, py, _) = seen.project(p);
            assert!(
                (px - (f64::from(w) / 2.0 + x)).abs() < 1e-6 && (py - (f64::from(h) / 2.0 - y)).abs() < 1e-6,
                "{p:?}: {px} {py} vs {x} {y}"
            );
        }
        assert!(s.execute("camera", &json!({"view": "Camera 1", "tilt": [0, 1]})).is_err(), "one value: the vertical");
    }

    /// An orthographic camera's verticals stand vertical already: its tilt correction is left alone (greyed in Properties),
    /// and its cone on the plan shows no far clip, which only a perspective camera has.
    #[test]
    fn an_orthographic_camera_takes_no_tilt_correction_and_shows_no_far_clip() {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 9000.0], "target": [0.0, 0.0, 0.0], "projection": "orthographic"})).unwrap();
        let plain = Camera::through(&view(&s, "Camera 1").camera.unwrap(), 35.0, false, 400, 300);
        s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": true, "clip": true, "far": 5000})).unwrap();
        let v = view(&s, "Camera 1");
        let tilted = Camera::through(&v.camera.unwrap(), 35.0, false, 400, 300);
        assert_eq!(Camera { clip: None, ..tilted }, Camera { clip: None, ..plain }, "the same picture");
        assert!(!crate::render::camera_cone(&v).unwrap().clipped, "no far clip drawn");
    }

    /// With the tilt corrected the far clip plane stands upright, as far in front of the eye as it says: the cone on the plan
    /// reaches that far, which is what it clips.
    #[test]
    fn with_the_tilt_corrected_the_cone_reaches_as_far_as_the_far_plane_clips() {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 11600.0], "target": [0.0, 0.0, 1600.0], "clip": true, "far": 8000})).unwrap();
        let tilted = crate::render::camera_cone(&view(&s, "Camera 1")).unwrap();
        assert!((tilted.reach - 8000.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6, "looking down 45°: {}", tilted.reach);
        s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": true})).unwrap();
        let upright = crate::render::camera_cone(&view(&s, "Camera 1")).unwrap();
        assert!((upright.reach - 8000.0).abs() < 1e-6, "{}", upright.reach);
    }

    /// Navigation is not on the undo stack, as Revit's: a view's orientation written back makes no undo step, the project is
    /// changed, and undoing an edit made before leaves the view where it was turned to.
    #[test]
    fn a_view_s_orientation_is_no_undo_step_and_undo_leaves_it_where_it_was_turned() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0.0, 0.0], [6000.0, 0.0]]})).unwrap();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 1600.0], "target": [0.0, 0.0, 1600.0]})).unwrap();
        s.execute("wall", &json!({"points": [[0.0, 3000.0], [6000.0, 3000.0]]})).unwrap();
        let steps = s.history().len();
        let before = std::sync::Arc::clone(&s.model);
        let p = json!({"view": "{3D}", "azimuth": 30, "elevation": 20, "target": [3000.0, 2000.0, 0.0], "across": 9000});
        s.execute("view_orientation", &p).unwrap();
        s.execute("view_orientation", &json!({"view": "Camera 1", "eye": [500.0, -9000.0, 1600.0], "target": [500.0, 0.0, 1500.0]}))
            .unwrap();
        assert_eq!(s.history().len(), steps, "no undo step");
        assert!(!std::sync::Arc::ptr_eq(&before, &s.model), "the project changed");
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.elements.values().filter(|e| e.category() == "wall").count(), 1, "the wall undone");
        assert_eq!(view(&s, "{3D}").framing.map(|f| f.across), Some(9000.0), "the default view stays where it was turned");
        assert_eq!(view(&s, "Camera 1").camera.unwrap().eye, [500.0, -9000.0, 1600.0], "the camera stays where it flew");
        s.execute("redo", &json!({})).unwrap();
        assert_eq!(view(&s, "Camera 1").camera.unwrap().eye, [500.0, -9000.0, 1600.0]);
        // A camera's own settings changed in Properties, or by the agent, are undone as ever.
        s.execute("camera", &json!({"view": "Camera 1", "eye_elevation": 2500})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(view(&s, "Camera 1").camera.unwrap().eye[2], 1600.0);
    }
}

mod orientation_tests {
    use serde_json::json;

    use crate::engine::Session;

    fn house() -> Session {
        let mut s = Session::new();
        s.execute("floor", &json!({"boundary": [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0]]})).unwrap();
        s.execute("wall", &json!({"points": [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0]]})).unwrap();
        s
    }

    fn png(s: &mut Session, p: serde_json::Value) -> serde_json::Value {
        s.execute("render", &p).unwrap()["pngBase64"].clone()
    }

    /// Every 3D view keeps where it was left, as Revit's do: `render` of it draws that, not the default fitted view, and a
    /// view's own graphics and section box are in its picture.
    #[test]
    fn a_3d_view_renders_where_it_was_left_with_its_own_graphics_and_box() {
        let mut s = house();
        let fitted = png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160}));
        let r = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap();
        assert_eq!(r["fitted"].as_bool(), Some(true));
        let before = r["revision"].clone();
        let r = s
            .execute(
                "view_orientation",
                &json!({"view": "{3D}", "azimuth": 30, "elevation": 20, "target": [3000.0, 2000.0, 0.0], "across": 9000}),
            )
            .unwrap();
        assert_eq!((r["azimuth"].as_f64(), r["across"].as_f64(), r["fitted"].as_bool()), (Some(30.0), Some(9000.0), Some(false)));
        assert_ne!(r["revision"], before, "the revision follows the orientation");
        let left = png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160}));
        assert_ne!(left, fitted, "it draws where it was left");
        assert_eq!(png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160})), left, "and does so again");
        // Its own visual style and section box change the picture and the revision.
        let rev = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap()["revision"].clone();
        s.execute("visual_style", &json!({"view": "{3D}", "style": "wireframe"})).unwrap();
        let styled = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap();
        assert_eq!(styled["style"].as_str(), Some("Wireframe"));
        assert_ne!(styled["revision"], rev);
        assert_ne!(png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160})), left);
        // A camera view's own box is its own: the default's leaves it alone.
        s.execute("camera", &json!({"eye": [-4000.0, -5000.0, 1600.0], "target": [3000.0, 2000.0, 1000.0]})).unwrap();
        let cam = png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160}));
        s.execute("section_box", &json!({"view": "{3D}", "min": [0.0, 0.0, 0.0], "max": [1000.0, 1000.0, 500.0]})).unwrap();
        assert_eq!(png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160})), cam, "the default's box is not the camera's");
        s.execute("section_box", &json!({"view": "Camera 1", "min": [0.0, 0.0, 0.0], "max": [1000.0, 1000.0, 500.0]})).unwrap();
        assert_ne!(png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160})), cam, "its own box cuts it");
        let read = s.execute("view_orientation", &json!({"view": "Camera 1"})).unwrap();
        assert!(read["camera"]["eye"].is_array() && read["section_box"]["on"].as_bool() == Some(true), "{read}");
        // Fitted again on asking, and refused nonsense.
        let r = s.execute("view_orientation", &json!({"view": "{3D}", "fitted": true})).unwrap();
        assert_eq!(r["fitted"].as_bool(), Some(true));
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "across": 5000})).is_err(), "a target is needed too");
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "elevation": 120})).is_err());
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "spin": 1})).is_err());
    }
}
