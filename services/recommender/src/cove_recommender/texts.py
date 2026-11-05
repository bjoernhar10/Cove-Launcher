"""UI language of the reasons the recommender writes ("de" or "en").

The core sends `system.setLanguage` before its first request and after every
change; until then the texts are German.
"""

from __future__ import annotations

from typing import Literal

type Language = Literal["de", "en"]

_state: dict[str, Language] = {"language": "de"}

GENRE_LABELS: dict[Language, dict[str, str]] = {
    "de": {
        "action": "Action",
        "adventure": "Adventure",
        "rpg": "Rollenspiel",
        "strategy": "Strategie",
        "simulation": "Simulation",
        "puzzle": "Puzzle",
        "platformer": "Platformer",
        "shooter": "Shooter",
        "racing": "Rennspiel",
        "sports": "Sport",
        "horror": "Horror",
        "indie": "Indie",
    },
    "en": {
        "action": "Action",
        "adventure": "Adventure",
        "rpg": "Role-playing",
        "strategy": "Strategy",
        "simulation": "Simulation",
        "puzzle": "Puzzle",
        "platformer": "Platformer",
        "shooter": "Shooter",
        "racing": "Racing",
        "sports": "Sports",
        "horror": "Horror",
        "indie": "Indie",
    },
}


def set_language(language: Language) -> None:
    _state["language"] = language


def language() -> Language:
    return _state["language"]


def t(de: str, en: str) -> str:
    """The text in the current language."""
    return en if language() == "en" else de


def genre_label(genre: str) -> str:
    return GENRE_LABELS[language()].get(genre, genre)


def hours(minutes: int) -> str:
    """Playtime in hours with one decimal below 10 ("4,5" in German, "4.5" in English)."""
    value = minutes / 60
    if value < 10:
        text = f"{value:.1f}".removesuffix(".0")
        return text.replace(".", ",") if language() == "de" else text
    return str(round(value))
