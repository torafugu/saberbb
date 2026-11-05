"""Frozen ID-only diagnostic assignments and per-batter decision provenance (no DB access)."""
from collections import Counter, defaultdict
from rust_observed_intent_audit import decide

STRATEGIES = ('let_batter_decide', 'put_ball_in_play', 'seek_line_drive')
PROFILES = ('default', 'mixed-id-v1')


def settings_for(profile, schedule):
    if profile not in PROFILES:
        raise ValueError('unsupported_batting_profile')
    if profile == 'default':
        return dict(away_strategy='let_batter_decide', home_strategy='let_batter_decide', player_preferences={})
    settings = dict(player_preferences={})
    for side in ('away', 'home'):
        team = schedule[side + '_team']
        if type(team['id']) is not int or not 0 <= team['id'] <= 65535:
            raise ValueError('invalid_profile_team_id')
        settings[side + '_strategy'] = STRATEGIES[team['id'] % 3]
        for player in team['players']:
            pid = player['info']['id']
            if type(pid) is not int or not 0 < pid < 2**63 or str(pid) in settings['player_preferences']:
                raise ValueError('invalid_or_duplicate_profile_player_id')
            settings['player_preferences'][str(pid)] = dict(preferred_intent='line_drive' if pid % 2 else 'contact_first')
    return settings


class BatterAudit:
    def __init__(self):
        self.coverage = Counter(states=0, missing_batter_context=0, verified_decisions=0,
                                missing_observed_decision=0, comparable_pitches=0)
        self.trace = []
        self.groups = defaultdict(Counter)

    def observe(self, game, seq, state, case, profile):
        self.coverage['states'] += 1
        context = state.get('intent_batter')
        if context is None:
            self.coverage['missing_batter_context'] += 1
            if profile != 'default':
                raise ValueError('mixed_profile_missing_batter_context')
            return None
        if set(context) != {'batter_id', 'is_top'} or type(context['batter_id']) is not int or type(context['is_top']) is not bool:
            raise ValueError('invalid_batter_context')
        top = context['is_top']; pid = context['batter_id']; side = 'away' if top else 'home'
        schedule = case['schedule']; team = schedule[side + '_team']
        ids = [p['info']['id'] for p in team['players']]
        other_ids = [p['info']['id'] for p in schedule[('home' if top else 'away') + '_team']['players']]
        if ids.count(pid) != 1 or pid in other_ids:
            raise ValueError('batter_context_roster_mismatch')
        settings = case['batting_settings']
        strategy = settings[side + '_strategy']
        preference = settings['player_preferences'].get(str(pid), dict(preferred_intent='contact_first'))['preferred_intent']
        decision = decide(strategy, preference)
        observed = state.get('observed_intent_plans')
        if observed is not None:
            if (observed['game_id'], observed['count_seq']) != (game, seq) or observed['decision'] != decision:
                raise ValueError('frozen_batter_decision_mismatch')
            self.coverage['verified_decisions'] += 1
        else:
            self.coverage['missing_observed_decision'] += 1
        key = (team['id'], pid, strategy, preference, decision['selected_intent'], decision['reason'])
        self.coverage['decision:' + strategy + ':' + preference] += observed is not None
        counts = self.groups[key]; counts['states'] += 1; counts['verified_decisions'] += observed is not None
        counts['manager_override'] += observed is not None and decision['selected_intent'] != preference
        self.trace.append(dict(game_id=game, count_seq=seq, team_id=team['id'], batter_id=pid, is_top=top,
            strategy=strategy, preferred_intent=preference, expected_selected_intent=decision['selected_intent'],
            selected_intent=observed['decision']['selected_intent'] if observed is not None else None,
            decision_reason=observed['decision']['reason'] if observed is not None else None,
            decision_status='verified' if observed is not None else 'missing_observed_decision'))
        return key

    def outcomes(self, key, rows):
        if key is None or not rows:
            return
        selected = [r for r in rows if r['selected']]
        if len(selected) != 1 or selected[0]['intent'] != key[4]:
            raise ValueError('batter_selected_candidate_mismatch')
        counts = self.groups[key]; counts['comparable_pitches'] += 1
        self.coverage['comparable_pitches'] += 1
        row = selected[0]
        counts['searched'] += row['status'] in {'contact', 'no_contact', 'unresolved', 'overlap_at_start'}
        for event in ('geometry', 'successful', 'fair', 'downward', 'high_vla', 'line_window'):
            counts[event] += row[event]

    def summaries(self):
        rows = []
        for key, counts in sorted(self.groups.items()):
            row = dict(zip(('team_id', 'batter_id', 'strategy', 'preferred_intent', 'selected_intent', 'decision_reason'), key))
            row.update({k: counts[k] for k in ('states', 'verified_decisions', 'manager_override', 'comparable_pitches',
                'searched', 'geometry', 'successful', 'fair', 'downward', 'high_vla', 'line_window')})
            for event in ('successful', 'fair', 'downward', 'high_vla', 'line_window'):
                row[event + '_pct_per_comparable_pitch'] = 100 * counts[event] / counts['comparable_pitches'] if counts['comparable_pitches'] else None
            rows.append(row)
        return rows
