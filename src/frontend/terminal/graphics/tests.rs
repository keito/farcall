use super::*;

fn display_size(width: u16, height: u16) -> (usize, usize) {
    calc_display_size(Winsize {
        ws_row: 25,
        ws_col: 80,
        ws_xpixel: width,
        ws_ypixel: height,
    })
}

#[test]
fn width_limited_image_reserves_padding_and_preserves_aspect_ratio() {
    assert_eq!(display_size(816, 1016), (800, 600));
}

#[test]
fn height_limited_image_reserves_padding_and_preserves_aspect_ratio() {
    assert_eq!(display_size(2016, 616), (800, 600));
}

#[test]
fn large_terminal_sizes_cap_both_image_dimensions() {
    assert_eq!(display_size(1295, 976), (1279, 959));
    assert_eq!(display_size(1296, 976), (1280, 960));
    assert_eq!(display_size(1297, 976), (1280, 960));
    assert_eq!(display_size(u16::MAX, u16::MAX), (1280, 960));
}

#[test]
fn terminals_without_room_for_padding_produce_zero_size() {
    for dimension in [0, 1, 15, 16] {
        assert_eq!(display_size(dimension, u16::MAX), (0, 0));
        assert_eq!(display_size(u16::MAX, dimension), (0, 0));
    }
    assert_eq!(display_size(0, 0), (0, 0));
}

#[test]
fn tiny_images_round_down_without_exceeding_available_space() {
    assert_eq!(display_size(17, u16::MAX), (1, 0));
    assert_eq!(display_size(u16::MAX, 17), (1, 1));
}

#[test]
fn image_dimensions_stay_bounded_and_fit_available_space() {
    for width in 0..=u16::MAX {
        for height in [0, 15, 16, 17, 18, 200, 600, 976, u16::MAX] {
            let (image_width, image_height) = display_size(width, height);
            assert!(image_width <= MAX_DISPLAY_WIDTH);
            assert!(image_height <= 960);
            assert!(image_width <= usize::from(width.saturating_sub(16)));
            assert!(image_height <= usize::from(height.saturating_sub(16)));
        }
    }
}
