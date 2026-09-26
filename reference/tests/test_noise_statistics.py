import random
import unittest

import noise_statistics


class KolmogorovSmirnovTests(unittest.TestCase):
    def test_same_distribution_passes_and_shifted_distribution_fails(self):
        rng = random.Random(4)
        first = [rng.gauss(0, 1) for _ in range(2000)]
        second = [rng.gauss(0, 1) for _ in range(2000)]
        shifted = [rng.gauss(0.3, 1) for _ in range(2000)]
        statistic, p = noise_statistics.ks_two_sample(first, first)
        self.assertEqual((statistic, p), (0.0, 1.0))
        self.assertGreater(noise_statistics.ks_two_sample(first, second)[1], 0.01)
        self.assertLess(noise_statistics.ks_two_sample(first, shifted)[1], 1e-6)


if __name__ == "__main__":
    unittest.main()
