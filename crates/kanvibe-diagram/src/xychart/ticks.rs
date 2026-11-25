//! d3-array `ticks(start, stop, count)`: 1·2·5·10 배 간격의 보기 좋은 눈금 값.

const E10: f64 = 7.071_067_811_865_476;
const E5: f64 = 3.162_277_660_168_379_5;
const E2: f64 = std::f64::consts::SQRT_2;

/// JS `Math.round`(0.5는 위로)
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// tickSpec: (첫 번호, 끝 번호, 간격). 간격이 음수면 그 역수가 실제 간격이다
fn tick_spec(start: f64, stop: f64, count: f64) -> (f64, f64, f64) {
    let step = (stop - start) / count.max(0.0);
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= E10 {
        10.0
    } else if error >= E5 {
        5.0
    } else if error >= E2 {
        2.0
    } else {
        1.0
    };
    let (mut first, mut last, increment);
    if power < 0.0 {
        let inverse = 10f64.powf(-power) / factor;
        first = js_round(start * inverse);
        last = js_round(stop * inverse);
        if first / inverse < start {
            first += 1.0;
        }
        if last / inverse > stop {
            last -= 1.0;
        }
        increment = -inverse;
    } else {
        increment = 10f64.powf(power) * factor;
        first = js_round(start / increment);
        last = js_round(stop / increment);
        if first * increment < start {
            first += 1.0;
        }
        if last * increment > stop {
            last -= 1.0;
        }
    }
    if last < first && (0.5..2.0).contains(&count) {
        return tick_spec(start, stop, count * 2.0);
    }
    (first, last, increment)
}

/// 정의역 방향(`stop < start`면 내림차순)대로 눈금 값을 돌려준다
pub fn linear_ticks(start: f64, stop: f64, count: usize) -> Vec<f64> {
    let count = count as f64;
    if count <= 0.0 {
        return Vec::new();
    }
    if start == stop {
        return vec![start];
    }
    let is_reverse = stop < start;
    let (first, last, increment) = if is_reverse {
        tick_spec(stop, start, count)
    } else {
        tick_spec(start, stop, count)
    };
    if last < first || last.is_nan() || first.is_nan() {
        return Vec::new();
    }
    let total = (last - first) as usize + 1;
    let value_at = |number: f64| {
        if increment < 0.0 {
            number / -increment
        } else {
            number * increment
        }
    };
    (0..total)
        .map(|index| {
            let offset = index as f64;
            if is_reverse {
                value_at(last - offset)
            } else {
                value_at(first + offset)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기준_차트와_같은_눈금을_고른다() {
        assert_eq!(
            linear_ticks(11000.0, 4000.0, 10),
            (0..15)
                .map(|index| 11000.0 - 500.0 * f64::from(index))
                .collect::<Vec<_>>()
        );
        assert_eq!(linear_ticks(1.0, 6.0, 10)[1], 1.5);
        assert_eq!(linear_ticks(0.0, 50.0, 10).len(), 11);
    }
}
