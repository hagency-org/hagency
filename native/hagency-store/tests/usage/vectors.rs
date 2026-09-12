use super::*;

#[test]
fn native_usage_high_water_vectors() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/usage-vectors.json")).unwrap();
    for vector in vectors["vectors"].as_array().unwrap() {
        let mut f = Fixture::new(Framework::Claude);
        let (_, _, source) = f.start();
        for (i, step) in vector["observations"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let c = &step["totals"];
            let observation = claude(
                c["input"].as_u64().unwrap(),
                c["output"].as_u64().unwrap(),
                c["cacheWrite"].as_u64().unwrap(),
                c["cacheRead"].as_u64().unwrap(),
            );
            let at = step["at"].as_u64().unwrap();
            f.db.record_usage_observation(&source, &format!("observation_{i}"), &observation, at)
                .unwrap();
            let expected = &step["expected"];
            let source = f.db.usage_source(&source).unwrap();
            assert_eq!(
                serde_json::to_value(source.high_water).unwrap(),
                expected["highWater"]
            );
            assert_eq!(
                source.regressions,
                expected["regressions"].as_u64().unwrap()
            );
            let known = f.totals().known_high_water_lower_bound.unwrap();
            assert_eq!(
                known.observed_display_lower_bound().unwrap(),
                expected["total"].as_u64().unwrap()
            );
            assert_eq!(
                known.observed_fresh_lower_bound().unwrap(),
                expected["fresh"].as_u64().unwrap()
            );
            for (kind, name) in [
                (UsagePeriodKind::Daily, "daily"),
                (UsagePeriodKind::Monthly, "monthly"),
            ] {
                let period = f.db.usage_period(&f.engagement, kind, at).unwrap().unwrap();
                assert_eq!(period.key, expected[name]["key"]);
                assert_eq!(
                    serde_json::to_value(period.observed_growth).unwrap(),
                    expected[name]["totals"]
                );
                assert_eq!(
                    period
                        .known_growth_lower_bound
                        .observed_display_lower_bound()
                        .unwrap(),
                    expected[name]["total"].as_u64().unwrap()
                );
            }
        }
    }
}
