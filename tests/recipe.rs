//! The parameters a fixture is rendered from, and the axes a report bands by.

use motif::fixtures::{Axis, Drift, Recipe, Texture};

fn clicks(tempo: f64, meter: usize, drift: Drift) -> Recipe {
    Recipe {
        tempo,
        meter,
        bars: 4,
        drift,
        texture: Texture::Percussion {
            sharpness: 1.0,
            density: 1,
            dropout: 0.0,
            syncopation: 0.0,
        },
    }
}

fn pitched(texture: Texture) -> Recipe {
    Recipe {
        tempo: 150.0,
        meter: 4,
        bars: 4,
        drift: Drift::Steady,
        texture,
    }
}

fn level(axis: Axis, recipe: &Recipe) -> String {
    axis.level(recipe)
        .unwrap_or_else(|| panic!("{} applies", axis.named()))
}

#[test]
fn every_axis_is_named_and_no_two_share_a_name() {
    let mut names: Vec<_> = Axis::ALL.iter().map(|axis| axis.named()).collect();
    let count = names.len();
    names.sort_unstable();
    names.dedup();

    assert_eq!(names.len(), count, "{names:?}");
    assert!(names.iter().all(|name| !name.is_empty()));
}

#[test]
fn a_tempo_band_is_named_for_its_beats_per_minute() {
    let recipe = clicks(120.0, 4, Drift::Steady);

    assert!(
        level(Axis::Tempo, &recipe).contains("120"),
        "{}",
        level(Axis::Tempo, &recipe)
    );
}

#[test]
fn tempo_bands_sort_into_tempo_order() {
    let named = |tempo| level(Axis::Tempo, &clicks(tempo, 4, Drift::Steady));

    assert!(named(90.0) < named(120.0));
    assert!(named(120.0) < named(150.0));
}

#[test]
fn a_meter_band_is_named_as_a_time_signature() {
    assert_eq!(level(Axis::Meter, &clicks(120.0, 3, Drift::Steady)), "3/4");
    assert_eq!(level(Axis::Meter, &clicks(120.0, 4, Drift::Steady)), "4/4");
}

#[test]
fn a_drift_band_is_named_for_its_kind_not_its_size() {
    let ramp = |to| level(Axis::Drift, &clicks(100.0, 4, Drift::Ramp { to }));

    assert_eq!(
        level(Axis::Drift, &clicks(100.0, 4, Drift::Steady)),
        "steady"
    );
    assert_eq!(ramp(140.0), "ramp");
    assert_eq!(ramp(160.0), "ramp");
    assert_eq!(
        level(Axis::Drift, &clicks(100.0, 4, Drift::Rubato { pull: 0.13 })),
        "rubato"
    );
}

#[test]
fn a_share_band_sorts_from_least_to_most() {
    let sharing = |syncopation| {
        level(
            Axis::Syncopation,
            &Recipe {
                texture: Texture::Percussion {
                    sharpness: 1.0,
                    density: 1,
                    dropout: 0.0,
                    syncopation,
                },
                ..clicks(120.0, 4, Drift::Steady)
            },
        )
    };

    assert!(sharing(0.0) < sharing(0.25));
    assert!(sharing(0.25) < sharing(0.5));
}

#[test]
fn a_percussive_axis_does_not_band_a_pitched_fixture() {
    for texture in [Texture::Chords, Texture::Line] {
        let recipe = pitched(texture);

        for axis in [
            Axis::Sharpness,
            Axis::Density,
            Axis::Dropout,
            Axis::Syncopation,
        ] {
            assert_eq!(axis.level(&recipe), None, "{} of {texture:?}", axis.named());
        }
    }
}

#[test]
fn the_grid_axes_band_a_pitched_fixture_as_they_band_any_other() {
    let recipe = pitched(Texture::Chords);

    for axis in [Axis::Tempo, Axis::Meter, Axis::Drift] {
        assert!(axis.level(&recipe).is_some(), "{}", axis.named());
    }
}

#[test]
fn every_axis_bands_a_percussive_fixture() {
    let recipe = clicks(120.0, 4, Drift::Steady);

    for axis in Axis::ALL {
        assert!(axis.level(&recipe).is_some(), "{}", axis.named());
    }
}
