import os
import subprocess
import sys
import unittest
from pathlib import Path


class RuntimeEnvTests(unittest.TestCase):
    def test_importing_prompt_tools_never_reads_an_operator_env(self):
        script = """
import os
from unittest.mock import patch
with patch('dotenv.load_dotenv', side_effect=AssertionError('Operator env was read')):
    import agent
assert not os.getenv('SPATIUS_API_KEY')
assert agent.INSTRUCTIONS
"""
        env = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP") if key in os.environ}
        result = subprocess.run([sys.executable, "-c", script], cwd=Path(__file__).resolve().parent,
                                env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, "Worker import failed or read operator configuration")
