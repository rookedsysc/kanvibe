#!/usr/bin/env python3
"""diff.py의 pixelmatch YIQ 색 차이를 pixelmatch 정의에서 손으로 계산한 값과 맞춰 본다.

실행: python3 parity/tools/screens/test_diff.py
"""
from __future__ import annotations

import itertools
import math
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from diff import HAS_PIL, MAX_YIQ_DELTA, compare_images, max_allowed_delta, yiq_color_delta  # noqa: E402

WHITE, BLACK = (255, 255, 255), (0, 0, 0)


class YiqColorDeltaTest(unittest.TestCase):
    def test_identical_pixels_have_no_delta(self):
        self.assertEqual(yiq_color_delta((12, 34, 56), (12, 34, 56)), 0.0)

    def test_white_against_black_is_the_luma_term_only(self):
        # 회색끼리는 I·Q가 0이라 0.5053 × 255² = 32857.1325만 남는다. 앞 픽셀이 밝으면 음수다
        self.assertAlmostEqual(yiq_color_delta(WHITE, BLACK), -32857.13, places=1)
        self.assertAlmostEqual(yiq_color_delta(BLACK, WHITE), 32857.13, places=1)

    def test_default_threshold_tolerates_gray_steps_up_to_26(self):
        # threshold 0.1 → 허용 거리 352.15. 회색 n단계 차이는 0.5053n²: 26단계 341.6, 27단계 368.4
        allowed = max_allowed_delta(0.1)
        self.assertAlmostEqual(allowed, 352.15, places=6)
        self.assertLessEqual(abs(yiq_color_delta((100, 100, 100), (126, 126, 126))), allowed)
        self.assertGreater(abs(yiq_color_delta((100, 100, 100), (127, 127, 127))), allowed)

    def test_pure_red_against_black(self):
        # Y=76.2183, I=151.9744, Q=53.9249 → 2935.40 + 6905.77 + 569.07 = 10410.25
        self.assertAlmostEqual(yiq_color_delta((255, 0, 0), BLACK), -10410.25, places=1)

    def test_max_delta_is_the_largest_distance_between_cube_corners(self):
        # pixelmatch의 35215는 RGB 정육면체 꼭짓점 사이 YIQ 거리 제곱의 최댓값(빨강↔청록 35214.75)을 올림한 값이다
        corners = list(itertools.product((0, 255), repeat=3))
        largest = max(abs(yiq_color_delta(a, b)) for a, b in itertools.combinations(corners, 2))
        self.assertAlmostEqual(yiq_color_delta((255, 0, 0), (0, 255, 255)), 35214.75, places=1)
        self.assertEqual(math.ceil(largest), MAX_YIQ_DELTA)


@unittest.skipUnless(HAS_PIL, "Pillow가 있어야 PNG를 만든다")
class CompareImagesTest(unittest.TestCase):
    def test_reports_strict_and_tolerant_ratios(self):
        from PIL import Image

        with tempfile.TemporaryDirectory() as directory:
            first_path, second_path = Path(directory, "a.png"), Path(directory, "b.png")
            first = Image.new("RGB", (4, 1), (100, 100, 100))
            second = first.copy()
            second.putpixel((0, 0), (110, 110, 110))  # 허용치 안
            second.putpixel((1, 0), (200, 200, 200))  # 허용치 밖
            first.save(first_path)
            second.save(second_path)

            tolerant = compare_images(first_path, second_path)
            strict = compare_images(first_path, second_path, strict=True)

        self.assertEqual((tolerant["strict_diff_pixels"], tolerant["tolerant_diff_pixels"]), (2, 1))
        self.assertEqual(tolerant["diff_ratio"], 0.25)
        self.assertEqual(strict["diff_ratio"], 0.5)


if __name__ == "__main__":
    unittest.main()
