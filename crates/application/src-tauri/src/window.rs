//! Desktop-window sizing that remains usable on small and scaled displays.

use tauri::{LogicalSize, Manager};

const PREFERRED_WIDTH: f64 = 1320.0;
const PREFERRED_HEIGHT: f64 = 860.0;
const MINIMUM_WIDTH: f64 = 760.0;
const MINIMUM_HEIGHT: f64 = 520.0;

// Leave room for taskbars, docks, and window-manager decorations. Tauri's
// monitor size is the complete display bounds rather than its work area.
const DISPLAY_WIDTH_FRACTION: f64 = 0.90;
const DISPLAY_HEIGHT_FRACTION: f64 = 0.88;

pub fn fit_main_window(app: &mut tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let monitor = window.current_monitor()?.or(window.primary_monitor()?);
    let Some(monitor) = monitor else {
        return Ok(());
    };

    let scale = monitor.scale_factor();
    let display = monitor.size();
    let logical_width = f64::from(display.width) / scale;
    let logical_height = f64::from(display.height) / scale;
    let size = startup_size(logical_width, logical_height);

    // A static minimum can prevent a window from fitting a small or heavily
    // scaled display. Keep normal minimums, but relax them when necessary.
    window.set_min_size(Some(LogicalSize::new(
        MINIMUM_WIDTH.min(size.width),
        MINIMUM_HEIGHT.min(size.height),
    )))?;
    window.set_size(size)?;
    window.center()?;
    Ok(())
}

fn startup_size(display_width: f64, display_height: f64) -> LogicalSize<f64> {
    LogicalSize::new(
        PREFERRED_WIDTH.min(display_width * DISPLAY_WIDTH_FRACTION),
        PREFERRED_HEIGHT.min(display_height * DISPLAY_HEIGHT_FRACTION),
    )
}

#[cfg(test)]
mod tests {
    use super::startup_size;

    #[test]
    fn preserves_the_preferred_size_on_large_displays() {
        let size = startup_size(1920.0, 1080.0);
        assert_eq!((size.width, size.height), (1320.0, 860.0));
    }

    #[test]
    fn shrinks_to_leave_room_on_small_displays() {
        let size = startup_size(1366.0, 768.0);
        assert_eq!(size.width, 1229.4);
        assert_eq!(size.height, 675.84);
    }

    #[test]
    fn uses_logical_dimensions_for_scaled_displays() {
        // A 1920x1080 display at 125% scaling is 1536x864 logical pixels.
        let size = startup_size(1536.0, 864.0);
        assert_eq!(size.width, 1320.0);
        assert_eq!(size.height, 760.32);
    }
}
