//! Tests for the sparkline renderer and the JSON/CSV serializers.

use econtoy::{sparkline, to_csv, to_json, Params, Simulation, SPARK_BLOCKS};

#[test]
fn sparkline_length_matches_input_when_short() {
    let data = [1.0, 2.0, 3.0, 4.0, 5.0];
    let spark = sparkline(&data, 40);
    assert_eq!(spark.chars().count(), data.len());
    for c in spark.chars() {
        assert!(SPARK_BLOCKS.contains(&c), "unexpected glyph {c}");
    }
}

#[test]
fn sparkline_downsamples_to_width() {
    let data: Vec<f64> = (0..100).map(f64::from).collect();
    let spark = sparkline(&data, 20);
    assert_eq!(spark.chars().count(), 20);
}

#[test]
fn sparkline_monotonic_data_ends_high_starts_low() {
    let data: Vec<f64> = (0..8).map(f64::from).collect();
    let spark = sparkline(&data, 8);
    let chars: Vec<char> = spark.chars().collect();
    assert_eq!(chars.first(), Some(&SPARK_BLOCKS[0]));
    assert_eq!(chars.last(), Some(&SPARK_BLOCKS[SPARK_BLOCKS.len() - 1]));
}

#[test]
fn sparkline_flat_series_is_uniform() {
    let data = [3.0; 6];
    let spark = sparkline(&data, 6);
    let chars: Vec<char> = spark.chars().collect();
    assert!(chars.iter().all(|&c| c == chars[0]));
    assert!(SPARK_BLOCKS.contains(&chars[0]));
}

#[test]
fn sparkline_empty_input_is_empty() {
    assert_eq!(sparkline(&[], 10), "");
    assert_eq!(sparkline(&[1.0, 2.0], 0), "");
}

#[test]
fn csv_shape_is_stable() {
    let params = Params {
        ticks: 30,
        ..Params::default()
    };
    let mut sim = Simulation::new(&params);
    sim.run();
    let csv = to_csv(sim.series());
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 31, "header + one row per tick");
    assert_eq!(
        lines[0],
        "tick,cpi,gdp,unemployment,inflation,avg_money,credit,broad_money"
    );
    // Every data row has the same number of columns as the header.
    let cols = lines[0].split(',').count();
    for row in &lines[1..] {
        assert_eq!(row.split(',').count(), cols);
    }
}

#[test]
fn json_shape_is_stable() {
    let params = Params {
        ticks: 20,
        ..Params::default()
    };
    let mut sim = Simulation::new(&params);
    let summary = sim.run();
    let json = to_json(&params, sim.series(), &summary);
    for key in [
        "\"params\"",
        "\"series\"",
        "\"summary\"",
        "\"cpi\"",
        "\"inflation\"",
        "\"base_money\"",
    ] {
        assert!(json.contains(key), "json missing {key}");
    }
    assert!(json.trim_start().starts_with('{'));
    assert!(json.trim_end().ends_with('}'));
    // Balanced braces and brackets.
    assert_eq!(json.matches('{').count(), json.matches('}').count());
    assert_eq!(json.matches('[').count(), json.matches(']').count());
}
