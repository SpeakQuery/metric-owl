use crate::model::Aggregate;
pub fn add(a: &mut Aggregate, value: Option<f64>, duration: f64, total: bool) {
    if let Some(v) = value.filter(|v| v.is_finite()) {
        a.min = Some(a.min.map_or(v, |x| x.min(v)));
        a.max = Some(a.max.map_or(v, |x| x.max(v)));
        if a.first.is_none() {
            a.first = Some(v);
        }
        a.last = Some(v);
        a.weighted_sum += v * duration;
        a.observed_seconds += duration;
        if total {
            a.total += v * duration;
        }
    }
}
pub fn merge(rows: &[Aggregate], resolution: u32) -> Vec<Aggregate> {
    let mut groups: std::collections::BTreeMap<(String, String, i64), Aggregate> =
        Default::default();
    for r in rows {
        let start = (r.start as i64 / resolution as i64) * resolution as i64;
        let a = groups
            .entry((r.source.clone(), r.metric.clone(), start))
            .or_insert_with(|| Aggregate {
                source: r.source.clone(),
                metric: r.metric.clone(),
                start: start as f64,
                end: (start + resolution as i64) as f64,
                resolution,
                ..Default::default()
            });
        if let Some(v) = r.min {
            a.min = Some(a.min.map_or(v, |x| x.min(v)));
        }
        if let Some(v) = r.max {
            a.max = Some(a.max.map_or(v, |x| x.max(v)));
        }
        if a.first.is_none() {
            a.first = r.first;
        }
        if r.last.is_some() {
            a.last = r.last;
        }
        a.weighted_sum += r.weighted_sum;
        a.observed_seconds += r.observed_seconds;
        a.total += r.total;
    }
    groups.into_values().collect()
}
/// Require seven days, >=90% time coverage, positive reasonably stable trend.
pub fn forecast_days(rows: &[Aggregate], capacity: f64) -> Option<f64> {
    if rows.len() < 7 || capacity <= 0. {
        return None;
    }
    let first = rows.first()?;
    let last = rows.last()?;
    let span = last.end - first.start;
    if span < 7. * 86400. || rows.iter().map(|r| r.observed_seconds).sum::<f64>() / span < 0.9 {
        return None;
    }
    let values: Vec<_> = rows
        .iter()
        .filter_map(|r| r.mean().map(|v| (r.start, v)))
        .collect();
    if values.len() < 7 {
        return None;
    }
    let n = values.len() as f64;
    let mx = values.iter().map(|x| x.0).sum::<f64>() / n;
    let my = values.iter().map(|x| x.1).sum::<f64>() / n;
    let xx = values.iter().map(|x| (x.0 - mx).powi(2)).sum::<f64>();
    let xy = values.iter().map(|x| (x.0 - mx) * (x.1 - my)).sum::<f64>();
    let yy = values.iter().map(|x| (x.1 - my).powi(2)).sum::<f64>();
    let slope = xy / xx;
    if slope <= 0. || xy * xy / (xx * yy) < 0.7 {
        return None;
    }
    let days = (capacity - last.last?) / slope / 86400.;
    (days.is_finite() && days >= 0.).then_some(days)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_peak_total_gaps() {
        let mut a = Aggregate {
            source: "host".into(),
            metric: "network_in".into(),
            start: 0.,
            end: 60.,
            resolution: 60,
            ..Default::default()
        };
        add(&mut a, Some(10.), 20., true);
        add(&mut a, None, 20., true);
        add(&mut a, Some(100.), 20., true);
        let r = merge(&[a], 300);
        assert_eq!(r[0].max, Some(100.));
        assert_eq!(r[0].total, 2200.);
        assert_eq!(r[0].observed_seconds, 40.);
        assert!(r[0].coverage() < 0.2);
    }
}
