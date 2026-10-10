"""Coverage guarantees for deliberately heterogeneous fuzz cases."""
import math
import random
import unittest

import fuzz_expr as fe
import fuzz_spec as fs


OPERATORS = {'cup', 'cap', 'difference', 'subseteq', 'in', 'SUBSET', 'UNION',
             'Cardinality', 'CHOOSE', 'application', 'DOMAIN', 'builder', 'map', 'EXCEPT'}
KINDS = {'rec', 'tup', 'set', 'fcn', 'str', 'int', 'mv'}


class HeterogeneousCoverage(unittest.TestCase):
    def test_expression_quota_and_operator_and_kind_coverage(self):
        for seed in (1, 17, 2026):
            g = fe.Gen(random.Random(seed))
            cases = [g.top() for _ in range(240)]
            mixed = [n for n in cases if n.tag.startswith('heterogeneous:')]
            self.assertEqual(len(mixed), 60)
            self.assertEqual({n.tag.split(':')[1] for n in mixed}, OPERATORS)
            kinds = {k.ty[0] for n in mixed for k in fe.descendants(n) if k.tag == 'heterogeneous-element'}
            self.assertEqual(kinds, KINDS)
            for n in mixed:
                self.assertEqual(fe.node_from_json(n.to_json()).render(), n.render())
                fe.shrink_candidates(n, random.Random(seed))

    def test_every_operator_reaches_every_kind_pair(self):
        g = fe.Gen(random.Random(4))
        seen = {op: set() for op in OPERATORS}
        tokens = {'cup': r'\cup', 'cap': r'\cap', 'difference': ' \\ ',
                  'subseteq': r'\subseteq', 'in': r'\in', 'SUBSET': 'SUBSET',
                  'UNION': 'UNION', 'Cardinality': 'Cardinality', 'CHOOSE': 'CHOOSE',
                  'application': ')[', 'DOMAIN': 'DOMAIN', 'builder': '{h \\in',
                  'map': '[h] : h \\in', 'EXCEPT': 'EXCEPT'}
        for i in range(14 * 21):
            n, inputs = fe.heterogeneous(g, i)
            op = n.tag.split(':')[1]
            self.assertIn(tokens[op], n.render())
            # Inspect the actual expression, not just unused helper inputs.
            kinds = frozenset(k.ty[0] for k in fe.descendants(n) if k.tag == 'heterogeneous-element')
            self.assertEqual(len(kinds), 2, n.render())
            seen[op].add(kinds)
            self.assertEqual(len({k.ty[0] for k in inputs['vF'].kids}), 2)
        self.assertTrue(all(len(pairs) == 21 for pairs in seen.values()), seen)

    def test_spec_quota_state_storage_and_operator_coverage(self):
        specs = [fs.SpecGen(17, i).spec() for i in range(240)]
        mixed = [s for s in specs if s.features.get('heterogeneous')]
        self.assertEqual(len(mixed), 60)
        self.assertEqual({s.features['heterogeneous'] for s in mixed}, OPERATORS)
        for s in mixed:
            self.assertEqual(len(s.features['heterogeneous_kinds']), 2, s.name)
            tla, cfg = fs.render(s)
            if 'vS' in dict(s.vars):
                self.assertIn('vS = {', tla)
                self.assertIn('vF = (', tla)
            else:
                self.assertIn('vLeft = ', tla)
                self.assertTrue({'vLeft', 'vRight'} <= s.actions[0].updates['r'][1].free)
            self.assertIn("pc' = 1", tla)
            self.assertIn("r' = ", tla)
            self.assertIn('pc = 0', tla)
            self.assertFalse(s.ok_guards)
            self.assertLessEqual(math.prod(fs.size(t) for _, t in s.vars), fs.UNIVERSE_CAP)
            self.assertEqual(fs.render(fs.SpecGen(17, int(s.name.split('_')[1])).spec()), (tla, cfg))



class KnownHeterogeneousFailures(unittest.TestCase):
    def test_only_documented_function_defects_are_expected(self):
        want = {'result': 'error', 'distinct': 1, 'generated': 1, 'depth': 1}
        got = {'result': 'success', 'distinct': 2, 'generated': 2, 'depth': 2}
        classify = lambda **kw: fs.heterogeneous_issue(
            want, kw.get('got', got), 'interp', 'difference', kw.get('diffs', ['result']),
            kw.get('message', 'Attempted to compare record:'), '', kw.get('kinds', ['rec', 'tup']))
        self.assertEqual(classify(), 'heterogeneous-function-comparison-defect')
        for args in ({'message': 'division by zero'}, {'kinds': ['rec', 'set']},
                     {'kinds': ['rec', 'rec']}, {'kinds': []}, {'diffs': ['paranoid']}, {'got': {'result': 'timeout'}}):
            self.assertIsNone(classify(**args))

    def test_compiled_disagreements_are_no_longer_expected(self):
        f = {'layer': 'compile-vs-interp', 'kind': 'state',
             'problem': 'paranoid: the interpreter fails (Attempted to compare the set x) but compiled code gives true',
             'node': {'t': ['set', ['mixed'], 2], 'k': []}}
        self.assertIsNone(fe.known_expression(f))

    def test_expression_defect_requires_function_operands(self):
        node = {'t': ['set', ['mixed'], 2], 'p': ['{', 0, ', ', 1, '}'],
                'k': [{'t': ['rec', []]}, {'t': ['tup', []]}]}
        f = {'layer': 'interp-vs-tlc', 'kind': 'error-vs-value', 'node': node,
             'tlc': ['err', 'Attempted to compare record: r with non-record f'], 'tlx': ['ok', '{}']}
        self.assertEqual(fe.known_expression(f), 'heterogeneous-function-comparison-defect')
        for changed in ({'tlx': ['err', 'division by zero']}, {'layer': 'compile-vs-interp'},
                        {'node': {**node, 'k': [{'t': ['rec', []]}, {'t': ['set', ['int'], 1]}]}}):
            self.assertIsNone(fe.known_expression({**f, **changed}))

    def test_live_and_json_probe_kinds_match(self):
        import json
        n, _ = fe.heterogeneous(fe.Gen(random.Random(1)), 0)
        self.assertEqual(fe.heterogeneous_function_kinds(n.to_json()),
                         fe.heterogeneous_function_kinds(json.loads(json.dumps(n.to_json()))))

    def test_function_domains_and_set_map_ranges_retain_the_operand_kinds(self):
        # These nodes have no mixed set literal: the former classifier lost
        # D-31 when functions were keys, or a set-map read a mixed range.
        g = fe.Gen(random.Random(212270154))
        covered = set()
        for i in range(14 * 21):
            n, inputs = fe.heterogeneous(g, i)
            expected = {k.ty[0] for k in inputs['vF'].kids}
            if 'rec' in expected and expected <= {'rec', 'tup', 'fcn'}:
                kinds = fe.heterogeneous_function_kinds(n.to_json())
                self.assertTrue(expected <= set(kinds), n.render())
                self.assertTrue(fe.function_comparison_defect(kinds, 'Attempted to compare record:'), n.render())
                covered.add(n.tag.split(':')[1])
        self.assertEqual(covered, OPERATORS)

    def test_unrelated_mapped_values_do_not_make_a_domain_error_expected(self):
        kids = [{'t': ['int', 2]}, {'t': ['rec', []]}, {'t': ['str']}, {'t': ['tup', []]}]
        node = {'t': ['fcn', ['mixed'], ['mixed'], 2], 'k': kids,
                'p': ['(', 0, ' :> ', 1, ' @@ ', 2, ' :> ', 3, ')']}
        kinds = fe.heterogeneous_function_kinds(node)
        self.assertEqual(kinds, ['int', 'str'])
        self.assertFalse(fe.function_comparison_defect(kinds, 'Attempted to compare integer with non-integer'))

    def test_same_domain_function_value_errors_are_not_defects(self):
        node = {'t': ['set', ['mixed'], 2], 'p': ['{', 0, ', ', 1, '}'],
                'k': [{'t': ['rec', [['a', ['int', 1]]]]},
                      {'t': ['fcn', ['str'], ['bool'], 1]}]}
        kinds = fe.heterogeneous_function_kinds(node)
        self.assertFalse(fe.function_comparison_defect(kinds, 'Attempted to compare integer with non-integer TRUE'))
        self.assertFalse(fe.function_comparison_defect(['tup', 'fcn'], 'Attempted to compare integer with non-integer TRUE'))


if __name__ == '__main__':
    unittest.main()
