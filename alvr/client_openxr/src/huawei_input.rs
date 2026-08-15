const HUAWEI_THUMBSTICK_CENTER: f32 = 0.5;
const HUAWEI_THUMBSTICK_RANGE_SCALE: f32 = 2.0;
const HUAWEI_THUMBSTICK_DEADZONE: f32 = 0.12;

pub fn normalize_thumbstick(raw_x: f32, raw_y: f32) -> (f32, f32) {
    let x = ((raw_x - HUAWEI_THUMBSTICK_CENTER) * HUAWEI_THUMBSTICK_RANGE_SCALE).clamp(-1.0, 1.0);
    let y = (-(raw_y - HUAWEI_THUMBSTICK_CENTER) * HUAWEI_THUMBSTICK_RANGE_SCALE).clamp(-1.0, 1.0);
    let magnitude = (x * x + y * y).sqrt();

    if magnitude <= HUAWEI_THUMBSTICK_DEADZONE {
        return (0.0, 0.0);
    }

    let remapped_magnitude = ((magnitude - HUAWEI_THUMBSTICK_DEADZONE)
        / (1.0 - HUAWEI_THUMBSTICK_DEADZONE))
        .clamp(0.0, 1.0);
    let scale = remapped_magnitude / magnitude;

    (x * scale, y * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_thumbstick(raw: (f32, f32), expected: (f32, f32)) {
        let actual = normalize_thumbstick(raw.0, raw.1);

        assert_near(actual.0, expected.0);
        assert_near(actual.1, expected.1);
    }

    #[test]
    fn centers_huawei_range_and_inverts_y() {
        assert_thumbstick((0.5, 0.5), (0.0, 0.0));
        assert_thumbstick((0.0, 0.5), (-1.0, 0.0));
        assert_thumbstick((1.0, 0.5), (1.0, 0.0));
        assert_thumbstick((0.5, 0.0), (0.0, 1.0));
        assert_thumbstick((0.5, 1.0), (0.0, -1.0));
    }

    #[test]
    fn removes_observed_idle_drift_for_both_controllers() {
        assert_thumbstick((0.4921208, 0.4589786), (0.0, 0.0));
        assert_thumbstick((0.48212357, 0.44993356), (0.0, 0.0));
    }

    #[test]
    fn applies_a_radial_deadzone_and_clamps_the_outer_range() {
        let (x, y) = normalize_thumbstick(1.0, 0.0);
        let magnitude = (x * x + y * y).sqrt();

        assert_near(magnitude, 1.0);
        assert_near(x, std::f32::consts::FRAC_1_SQRT_2);
        assert_near(y, std::f32::consts::FRAC_1_SQRT_2);

        let (x, y) = normalize_thumbstick(0.75, 0.5);
        assert!(x > 0.0 && x < 0.5);
        assert_near(y, 0.0);
    }
}
