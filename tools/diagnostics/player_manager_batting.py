"""Independent audit of player -> situational preferences -> manager -> intent (v1)."""
from execution_contact_posture import same
from rust_observed_intent_audit import decide

TENDENCIES = {
    'contact_oriented': (.8,.2,.4), 'line_drive_oriented': (.2,.8,.4),
    'adaptive': (.5,.5,1.), 'balanced': (.5,.5,.5),
}


def evaluate(batter_id, character, situation, manager):
    if type(batter_id) is not int or batter_id<=0 or character not in TENDENCIES:
        raise ValueError('invalid_player_batting_identity_or_character')
    if set(situation)!={'balls','strikes','outs','runners'}:
        raise ValueError('invalid_pre_pitch_situation_schema')
    for key,maximum in [('balls',3),('strikes',2),('outs',2)]:
        if type(situation[key]) is not int or not 0<=situation[key]<=maximum:
            raise ValueError('invalid_pre_pitch_batting_situation')
    if not isinstance(situation['runners'],list) or len(situation['runners'])!=3 or any(type(v) is not bool for v in situation['runners']):
        raise ValueError('invalid_pre_pitch_runner_presence')
    contact,line,adapt=TENDENCIES[character]
    protect=situation['strikes']==2
    advance=situation['runners'][2] and situation['outs']<2
    produce=(situation['balls']>=2 and situation['strikes']==0) or ((situation['runners'][1] or situation['runners'][2]) and situation['outs']==2)
    c=contact+adapt*((.5 if protect else 0.)+(.25 if advance else 0.))
    l=line+adapt*(.25 if produce else 0.)
    preference='line_drive' if l>c else 'contact_first'
    preferences=dict(contact_weight=c/(c+l),line_drive_weight=l/(c+l),preferred_intent=preference,
        two_strike_protection=protect,runner_advancement=advance,production_opportunity=produce)
    if manager is None:
        strategy,reason='let_batter_decide','no_manager'
    else:
        if (set(manager)!={'id','name','batting_policy'} or type(manager['id']) is not int or manager['id']<=0
            or not isinstance(manager['name'],str) or not manager['name'].strip()):
            raise ValueError('invalid_manager_identity')
        policy=manager['batting_policy']
        if policy in ('let_batter_decide','put_ball_in_play','seek_line_drive'):
            strategy,reason=policy,'standing_order'
        elif policy=='situational':
            if protect:strategy,reason='put_ball_in_play','two_strike_protection'
            elif advance:strategy,reason='put_ball_in_play','runner_advancement'
            elif produce:strategy,reason='seek_line_drive','run_production'
            else:strategy,reason='let_batter_decide','batter_discretion'
        else:raise ValueError('invalid_manager_policy')
    return dict(version=1,source='player_manager_batting_v1',batter_id=batter_id,character=character,
        tendencies=dict(contact_priority=contact,line_drive_priority=line,adaptability=adapt),situation=situation,
        preferences=preferences,manager=manager,instruction=dict(strategy=strategy,reason=reason),decision=decide(strategy,preference))


def audit_selection(state,case,batter_id,side,checks):
    trace=state.get('batting_selection')
    if trace is None:raise ValueError('missing_player_manager_batting_trace')
    team=case['schedule'][side+'_team']
    players=[p for p in team['players'] if p['info']['id']==batter_id]
    if len(players)!=1:raise ValueError('player_manager_roster_mismatch')
    expected=evaluate(batter_id,players[0].get('batting_character','balanced'),trace['situation'],team.get('manager'))
    same(expected,trace,checks,'player_manager_selection')
    return expected['decision']
