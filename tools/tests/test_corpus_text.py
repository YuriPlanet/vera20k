"""The committed corpus layout native_oracle.finish_vectors writes."""
import json
import unittest

from tools.native_oracle import corpus_text, shared_rng_states


class CorpusTextTests(unittest.TestCase):
    def test_rows_take_one_line_each_and_read_back_unchanged(self):
        data = {"rows": [{"cell": [1, 2], "rng": [3.5, -0.0]}, {"cell": [3, 4]}],
                "name": "x", "empty": {}, "draws": [7, 8]}
        text = corpus_text(data)
        self.assertEqual(text.splitlines(), [
            '{', ' "rows": [', '  {"cell":[1,2],"rng":[3.5,-0.0]},', '  {"cell":[3,4]}', ' ],',
            ' "name": "x",', ' "empty": {},', ' "draws": [7,8]', '}'])
        self.assertTrue(text.endswith("}\n"))
        self.assertEqual(json.loads(text), data)

    def test_repeated_rng_states_are_stored_once_in_first_met_order(self):
        first = {"disabled": False, "index_a": 0, "index_b": 31, "state": [1, 2]}
        second = dict(first, index_a=1)
        data = {"rows": [{"rng_before": first, "rng_after": second}, {"rng_before": second}],
                "seed": {"state": [5]}}
        self.assertEqual(shared_rng_states(data), {
            "rows": [{"rng_before": {"rng_state": 0}, "rng_after": {"rng_state": 1}},
                     {"rng_before": {"rng_state": 1}}],
            "seed": {"state": [5]}, "rng_states": [first, second]})


if __name__ == "__main__":
    unittest.main()
