import tempfile
import unittest
from pathlib import Path

from pitch_location_execution_audit import Settings


class SimulationConfigTest(unittest.TestCase):
    def test_defaults_and_overrides(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "default-config.toml"
            self.assertEqual(Settings.from_config(path), Settings(0.2, 0.0, None, 0.0, False))
            path.write_text("[simulation]\npitch_control_cap = 0.0\npitch_location_error_floor = 0.1\npitch_execution_fatigue = true\n")
            self.assertEqual(Settings.from_config(path), Settings(0.2, 0.1, 0.0, 0.0, True))

    def test_rejects_invalid_settings(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "default-config.toml"
            for setting in ["pitch_control_cap = inf", "pitch_location_error_floor = 0.6", "pitch_execution_fatigue = 1"]:
                path.write_text("[simulation]\n" + setting)
                with self.assertRaises(ValueError):
                    Settings.from_config(path)
