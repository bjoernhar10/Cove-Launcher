from cove_recommender import engine, texts
from cove_recommender.data import LibraryGame

from .fixtures import CATALOG, NOW, played

BY_ID = {g.id: g for g in CATALOG}


def ids(recs: list[engine.Recommendation]) -> list[str]:
    return [r.game_id for r in recs]


def genres_of(game_id: str) -> set[str]:
    return set(BY_ID[game_id].genres)


HK_FAN = [played("hk", "Hollow Knight", 40, favorite=True, achievements=(30, 63))]


def test_metroidvania_fan_gets_metroidvanias_not_racing() -> None:
    recs = engine.recommend_for_user(HK_FAN, CATALOG, 5, NOW)
    top = ids(recs)
    assert "hk" not in top, "owned games are excluded"
    assert top[0] == "silksong", top
    assert all("platformer" in genres_of(i) for i in top[:3]), top
    assert not {"forza5", "dirt"} & set(top)


def test_reason_names_the_game_that_drives_the_recommendation() -> None:
    recs = engine.recommend_for_user(HK_FAN, CATALOG, 3, NOW)
    assert recs[0].reason == "Weil du 40 Std. Hollow Knight gespielt hast"


def test_prior_keeps_obscure_low_rated_games_down() -> None:
    top = ids(engine.recommend_for_user(HK_FAN, CATALOG, len(CATALOG), NOW))
    assert top.index("blasphemous") < top.index("obscure")


def test_mmr_avoids_a_list_of_near_identical_games() -> None:
    top5 = ids(engine.recommend_for_user(HK_FAN, CATALOG, 5, NOW))
    clones = [i for i in top5 if i.startswith("clone")]
    assert len(clones) <= 2, top5


def test_abandoned_games_count_slightly_negative() -> None:
    witcher = played("witcher3", "The Witcher 3", 30)
    enjoyed_racing = [witcher, played("forza5", "Forza Horizon 5", 20)]
    abandoned_racing = [witcher, played("forza5", "Forza Horizon 5", 0.3, days_ago=30, sessions=1)]
    rank_when_enjoyed = ids(
        engine.recommend_for_user(enjoyed_racing, CATALOG, len(CATALOG), NOW)
    ).index("dirt")
    rank_when_abandoned = ids(
        engine.recommend_for_user(abandoned_racing, CATALOG, len(CATALOG), NOW)
    ).index("dirt")
    assert rank_when_abandoned > rank_when_enjoyed
    assert engine.interaction_weight(abandoned_racing[1], NOW) < 0


def test_recent_play_outweighs_old_play() -> None:
    library = [
        played("skyrim", "Skyrim", 100, days_ago=730),
        played("civ6", "Civilization VI", 20, days_ago=2),
    ]
    first = engine.recommend_for_user(library, CATALOG, 1, NOW)[0]
    assert "strategy" in genres_of(first.game_id), first


def test_weights() -> None:
    unplayed = played("hk", "Hollow Knight", 0, sessions=0)
    assert engine.interaction_weight(unplayed, NOW) == engine.UNPLAYED_WEIGHT
    favorite_unplayed = LibraryGame("hk", "Hollow Knight", 0, None, True, 0, 0, 0)
    assert engine.interaction_weight(favorite_unplayed, NOW) > engine.interaction_weight(
        unplayed, NOW
    )
    with_achievements = played("hk", "Hollow Knight", 5, achievements=(40, 40))
    assert engine.interaction_weight(with_achievements, NOW) > engine.interaction_weight(
        played("hk", "H", 5), NOW
    )
    # Short but recent and not yet "given up": not negative.
    assert (
        engine.interaction_weight(played("hk", "Hollow Knight", 0.2, days_ago=1, sessions=1), NOW)
        > 0
    )


def test_cold_start_uses_the_prior() -> None:
    recs = engine.recommend_for_user([], CATALOG, 3, NOW)
    assert recs[0].game_id == "portal2"
    assert recs[0].reason == "Sehr gut bewertet"


def test_candidate_restriction() -> None:
    recs = engine.recommend_for_user(
        HK_FAN, CATALOG, 10, NOW, candidate_ids=["dirt", "ori", "civ6"]
    )
    assert set(ids(recs)) == {"dirt", "ori", "civ6"}
    assert ids(recs)[0] == "ori"


def test_discover_suggests_unplayed_genres_close_to_the_profile() -> None:
    known = {"platformer", "action", "indie"}
    recs = engine.discover_new(HK_FAN, CATALOG, 4, NOW)
    assert recs
    for rec in recs:
        assert not genres_of(rec.game_id) & known, rec
        assert rec.reason.startswith("Neu für dich: ")
    # Fantasy adventures are close enough to a fantasy platformer; racing is too far away.
    order = ids(engine.discover_new(HK_FAN, CATALOG, 20, NOW))
    assert "skyrim" in order
    assert not {"dirt", "forza5"} & set(order)


def test_similar_games() -> None:
    recs = engine.similar_games("hk", CATALOG, 4)
    assert recs[0].game_id == "silksong"
    assert all(r.reason == "Ähnlich wie Hollow Knight" for r in recs)
    assert "hk" not in ids(recs)


def test_explain_variants() -> None:
    favorite_only = [LibraryGame("ori", "Ori and the Blind Forest", 30, NOW, True, 0, 0, 1)]
    assert engine.explain_recommendation("hk", favorite_only, CATALOG, NOW) == (
        "Weil Ori and the Blind Forest zu deinen Favoriten gehört"
    )
    short = [played("ori", "Ori and the Blind Forest", 0.5, sessions=3)]
    assert (
        engine.explain_recommendation("hk", short, CATALOG, NOW)
        == "Ähnlich wie Ori and the Blind Forest"
    )
    assert engine.explain_recommendation("hk", [played("ori", "Ori", 4.5)], CATALOG, NOW) == (
        "Weil du 4,5 Std. Ori gespielt hast"
    )


def test_explain_variants_in_english() -> None:
    texts.set_language("en")
    try:
        assert engine.explain_recommendation("hk", [played("ori", "Ori", 4.5)], CATALOG, NOW) == (
            "Because you played Ori for 4.5 h"
        )
        favorite_only = [LibraryGame("ori", "Ori", 30, NOW, True, 0, 0, 1)]
        assert engine.explain_recommendation("hk", favorite_only, CATALOG, NOW) == (
            "Because Ori is one of your favourites"
        )
    finally:
        texts.set_language("de")
