//! `--seed`: fill the local calendar with sample events (development aid).

use calen_core::{CalendarObject, Event, When};
use calen_store::{LOCAL_CALENDAR, Store};
use chrono::{Duration, Local, NaiveTime};
use std::path::Path;

pub fn seed(store: &Store, thumbs: &Path) {
    let today = Local::now().date_naive();
    let at = |d: i64, h: u32, m: u32| {
        let t = NaiveTime::from_hms_opt(h, m, 0).unwrap();
        When::at_local(calen_core::event::local_midnight(today + Duration::days(d)) + (t - NaiveTime::MIN))
    };
    let mut items: Vec<Event> = Vec::new();
    let mut e = Event::new("Project planning", at(0, 13, 0), at(0, 14, 0));
    e.location = "Google Meet".into();
    e.url = Some("https://meet.google.com/abc-defg-hij".into());
    items.push(e);
    let mut e = Event::new("Catch up over lunch", at(1, 12, 0), at(1, 13, 30));
    e.location = "DiBella's Old Fashioned Subs".into();
    e.url = Some("https://www.google.com/maps/place/DiBella's+Old+Fashioned+Subs/@43.08,-77.67,17z".into());
    e.geo = Some((43.08, -77.67));
    items.push(e);
    let mut e = Event::new("Connect Group", at(-6, 19, 0), at(-6, 21, 0));
    e.rrule = Some("FREQ=WEEKLY".into());
    items.push(e);
    items.push(Event::new("Check email", at(-1, 8, 0), at(-1, 8, 45)));
    items.push(Event::new("Band practice", at(2, 19, 0), at(2, 20, 0)));
    items.push(Event::new("Budget meeting", at(2, 10, 0), at(2, 11, 0)));
    items.push(Event::new("Design review", at(2, 10, 30), at(2, 12, 0)));
    let mut e = Event::new("Web Design Meeting", at(3, 11, 0), at(3, 12, 0));
    e.description = "Zoom: https://us02web.zoom.us/j/8812345678".into();
    items.push(e);
    items.push(Event::new("Lapor PLN Kutisari", When::Date(today + Duration::days(4)), When::Date(today + Duration::days(8))));
    items.push(Event::new("Grocery delivery", When::Date(today + Duration::days(1)), When::Date(today + Duration::days(2))));
    // An event with photos. The links point nowhere real; their thumbnails are
    // drawn here and put straight into the thumbnail cache, so nothing is fetched.
    let photos: Vec<String> = (0..3).map(|i| format!("https://img.example.invalid/trip-{}.png", i + 1)).collect();
    for (i, url) in photos.iter().enumerate() {
        write_photo(&calen_images::thumbnail_path(url, thumbs), i);
    }
    let mut e = Event::new("Weekend trip photos", at(-2, 16, 0), at(-2, 17, 0));
    e.location = "Bromo".into();
    e.description = calen_core::notes::join("Sunrise hike — the best shots from Saturday.", &photos);
    items.push(e);

    for ev in items {
        let obj = CalendarObject::new(ev);
        let _ = store.save_local(LOCAL_CALENDAR, obj.uid(), &obj.to_ics());
    }
}

/// A simple landscape: sky gradient, sun, two mountain ridges. `variant` picks
/// the colours.
fn write_photo(path: &Path, variant: usize) {
    const W: u32 = 256;
    const H: u32 = 256;
    type Rgb = [f32; 3];
    let (top, bottom, sun, far, near): (Rgb, Rgb, Rgb, Rgb, Rgb) = match variant % 3 {
        0 => ([40., 60., 130.], [250., 160., 110.], [255., 220., 150.], [90., 80., 130.], [40., 40., 70.]),
        1 => ([90., 170., 230.], [200., 230., 250.], [255., 250., 220.], [90., 140., 120.], [40., 90., 70.]),
        _ => ([20., 20., 50.], [120., 70., 120.], [250., 240., 200.], [60., 50., 90.], [20., 20., 40.]),
    };
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| image::Rgb([0, 1, 2].map(|i| (a[i] + (b[i] - a[i]) * t) as u8));
    let (sx, sy) = (160.0 + variant as f32 * 20.0, 120.0 - variant as f32 * 25.0);
    let img = image::RgbImage::from_fn(W, H, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let ridge_far = 150.0 + 25.0 * (fx / 37.0 + variant as f32).sin() + 12.0 * (fx / 13.0).sin();
        let ridge_near = 190.0 + 30.0 * (fx / 53.0 + 1.0 + variant as f32).sin() + 8.0 * (fx / 9.0).cos();
        if fy > ridge_near {
            mix(near, [0., 0., 0.], (fy - ridge_near) / 200.0)
        } else if fy > ridge_far {
            mix(far, near, (fy - ridge_far) / 120.0)
        } else if (fx - sx).hypot(fy - sy) < 22.0 {
            image::Rgb(sun.map(|v| v as u8))
        } else {
            mix(top, bottom, fy / ridge_far)
        }
    });
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let _ = img.save(path);
}
