"""Pure, deterministic Wispr Flow post-processing and post-insertion edit learner for Python."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Literal, TypedDict

FlowTone = Literal["casual", "work", "email", "code", "verbatim"]


class FlowOptions(TypedDict, total=False):
    tone: FlowTone
    stripFillers: bool
    resolveCorrections: bool
    formatLists: bool
    snippets: dict[str, str]
    vocabulary: list[str]
    selectedText: str


@dataclass(frozen=True)
class LearnedVocabularyDiff:
    added_terms: list[str] = field(default_factory=list)
    replacements: dict[str, str] = field(default_factory=dict)

    def to_contract_dict(self) -> dict[str, object]:
        return {
            "addedTerms": list(self.added_terms),
            "replacements": dict(self.replacements),
        }


PURE_FILLERS = ["um+", "uh+", "erm+", "ah+", "er", "hmm+"]
DISCOURSE_FILLERS = ["you know", "i guess", "sort of", "kind of", "like"]


def strip_fillers(text: str) -> str:
    """Remove spoken hesitations (um, uh, erm, ah) and comma/pause-flanked discourse fillers."""
    if not text:
        return text

    trimmed_lead = text.lstrip()
    started_uppercase = bool(
        trimmed_lead
        and trimmed_lead[0] == trimmed_lead[0].upper()
        and trimmed_lead[0] != trimmed_lead[0].lower()
    )

    out = text

    # 1. Discourse fillers when flanked by commas/pauses or at start/end with a comma
    for phrase in DISCOURSE_FILLERS:
        escaped = r"\s+".join(re.escape(part) for part in phrase.split())
        out = re.sub(
            rf"(^|[.!?]\s+)(?:{escaped})\s*,\s*",
            r"\1",
            out,
            flags=re.IGNORECASE,
        )
        out = re.sub(
            rf",\s*(?:{escaped})\s*,",
            ",",
            out,
            flags=re.IGNORECASE,
        )
        out = re.sub(
            rf",\s*(?:{escaped})(?=\s*(?:[.!?]|$))",
            "",
            out,
            flags=re.IGNORECASE,
        )

    # 2. Pure hesitation sounds anywhere as whole words, eating adjacent filler commas
    filler_alt = "|".join(PURE_FILLERS)
    out = re.sub(
        rf"(^|[.!?]\s+)(?:{filler_alt})(?:\s*,)?\s+",
        r"\1",
        out,
        flags=re.IGNORECASE,
    )
    out = re.sub(
        rf",\s*(?:{filler_alt})\s*,",
        "",
        out,
        flags=re.IGNORECASE,
    )
    out = re.sub(
        rf",\s*(?:{filler_alt})\b\s*",
        " ",
        out,
        flags=re.IGNORECASE,
    )
    out = re.sub(
        rf"\b(?:{filler_alt})\s*,\s*",
        " ",
        out,
        flags=re.IGNORECASE,
    )
    out = re.sub(
        rf"(?:^|\s+)\b(?:{filler_alt})\b(?=\s|[.!?,;:]|$)",
        " ",
        out,
        flags=re.IGNORECASE,
    )

    out = _normalize_inline_spacing(out)
    if started_uppercase and out:
        out = _capitalize_first_letter(out)
    return out


CORRECTION_MARKER_RE = re.compile(
    r"[,\s—-]*\b(no\s+wait|wait\s+no|actually\s+no|scratch\s+that|i\s+mean|or\s+rather)\b[,\s—-]*",
    re.IGNORECASE,
)

ANCHOR_WORDS = {
    "at",
    "to",
    "on",
    "in",
    "for",
    "with",
    "from",
    "by",
    "into",
    "about",
    "as",
    "of",
    "need",
    "needs",
    "want",
    "wants",
    "use",
    "using",
    "is",
    "are",
    "was",
    "were",
    "be",
    "call",
    "meet",
    "send",
    "deploy",
}

CLAUSE_STARTERS = {
    "i",
    "we",
    "you",
    "they",
    "he",
    "she",
    "it",
    "let",
    "let's",
    "lets",
    "please",
}


def resolve_course_corrections(text: str) -> str:
    """Resolve mid-sentence backtracking markers (no wait, actually no, scratch that, I mean)."""
    if not text:
        return text
    current = text

    for _ in range(8):
        match = CORRECTION_MARKER_RE.search(current)
        if not match:
            break

        before_raw = current[: match.start()].strip()
        after_raw = current[match.end() :].strip()
        if not before_raw or not after_raw:
            break

        sentence_boundary = re.match(r"^([^.!?]+[.!?]?)(.*)$", after_raw, flags=re.DOTALL)
        correction_part = (
            sentence_boundary.group(1) if sentence_boundary else after_raw
        ).strip()
        rest_after = (sentence_boundary.group(2) if sentence_boundary else "").strip()

        merged = _merge_correction(before_raw, correction_part)
        current = f"{merged} {rest_after}" if rest_after else merged

    return _normalize_inline_spacing(current)


def _merge_correction(before: str, after: str) -> str:
    before_tokens = before.split()
    after_tokens = after.split()
    first_after_clean = _clean_token(after_tokens[0] if after_tokens else "").lower()

    # Case 1: after starts with the same preposition/anchor near the end of before
    for i in range(len(before_tokens) - 1, max(-1, len(before_tokens) - 5), -1):
        if (
            _clean_token(before_tokens[i]).lower() == first_after_clean
            and first_after_clean in ANCHOR_WORDS
        ):
            prefix = " ".join(before_tokens[:i])
            return f"{prefix} {after}" if prefix else after

    # Case 2: after is a full replacement clause
    last_before_clean = _clean_token(before_tokens[-1] if before_tokens else "").lower()
    if (
        first_after_clean in CLAUSE_STARTERS
        and len(after_tokens) >= 3
        and last_before_clean not in ANCHOR_WORDS
    ):
        last_break = max(before.rfind(". "), before.rfind("! "), before.rfind("? "))
        if last_break != -1:
            kept = before[: last_break + 1].strip()
            return f"{kept} {after}"
        return after

    # Case 3: before ends with a time/number phrase after an anchor in the last 4 tokens
    for i in range(len(before_tokens) - 2, max(-1, len(before_tokens) - 5), -1):
        candidate = _clean_token(before_tokens[i]).lower()
        if candidate in ANCHOR_WORDS:
            prefix = " ".join(before_tokens[: i + 1])
            return f"{prefix} {after}"

    # Case 4: fallback replace trailing token(s)
    drop_count = min(len(before_tokens), max(1, min(2, len(after_tokens))))
    prefix = " ".join(before_tokens[: len(before_tokens) - drop_count])
    return f"{prefix} {after}" if prefix else after


ORDINAL_MAP = {
    "first": 1,
    "second": 2,
    "third": 3,
    "fourth": 4,
    "fifth": 5,
    "sixth": 6,
}


def format_spoken_lists(text: str) -> str:
    """Convert spoken list sequences (first X second Y third Z or bullet one X bullet two Y)."""
    if not text:
        return text

    bullet_numbered_re = re.compile(
        r"\bbullet\s+(?:one|two|three|four|five|six|\d+)\b[,\s:-]*",
        re.IGNORECASE,
    )
    bullet_matches = list(bullet_numbered_re.finditer(text))
    if len(bullet_matches) >= 2:
        return _build_list_from_matches(text, bullet_matches, lambda _m, _idx: "-")

    plain_bullet_re = re.compile(
        r"(?:^|\s)\bbullet\s+(?:point\s+)?[,\s:-]*",
        re.IGNORECASE,
    )
    plain_bullet_matches = list(plain_bullet_re.finditer(text))
    if len(plain_bullet_matches) >= 2:
        return _build_list_from_matches(text, plain_bullet_matches, lambda _m, _idx: "-")

    numbered_word_re = re.compile(
        r"\b(?:number|item)\s+(one|two|three|four|five|six|\d+)\b[,\s:-]*",
        re.IGNORECASE,
    )
    numbered_matches = list(numbered_word_re.finditer(text))
    if len(numbered_matches) >= 2:
        return _build_list_from_matches(
            text, numbered_matches, lambda _m, idx: f"{idx + 1}."
        )

    ordinal_re = re.compile(
        r"\b(first|second|third|fourth|fifth|sixth)\b[,\s:-]*",
        re.IGNORECASE,
    )
    raw_ordinal_matches = list(ordinal_re.finditer(text))
    sequential_ordinals: list[re.Match[str]] = []
    for m in raw_ordinal_matches:
        word = (m.group(1) or "").lower()
        expected = len(sequential_ordinals) + 1
        if ORDINAL_MAP.get(word) == expected:
            sequential_ordinals.append(m)

    if len(sequential_ordinals) >= 2:
        return _build_list_from_matches(
            text, sequential_ordinals, lambda _m, idx: f"{idx + 1}."
        )

    return text


def _build_list_from_matches(
    text: str,
    matches: list[re.Match[str]],
    marker_for,
) -> str:
    first_index = matches[0].start()
    lead_in_raw = re.sub(r"[,;]+$", "", text[:first_index].strip()).strip()

    items: list[str] = []
    for i, current in enumerate(matches):
        start = current.end()
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        sl = text[start:end].strip()
        sl = re.sub(r"^(?:and\s+|then\s+)", "", sl, flags=re.IGNORECASE)
        sl = re.sub(r"[,;.\s]+$", "", sl).strip()
        if sl:
            items.append(f"{marker_for(current, i)} {_capitalize_first_letter(sl)}")

    if not items:
        return text
    if not lead_in_raw:
        return "\n".join(items)
    lead_in = lead_in_raw if re.search(r"[:.!?]$", lead_in_raw) else f"{lead_in_raw}:"
    return f"{lead_in}\n" + "\n".join(items)


def expand_snippets(text: str, snippets: dict[str, str]) -> str:
    """Case-insensitive phrase trigger replacement on word boundaries (longest trigger wins)."""
    if not text or not snippets:
        return text

    entries = sorted(
        ((k.strip(), v) for k, v in snippets.items() if k.strip()),
        key=lambda kv: len(kv[0]),
        reverse=True,
    )
    if not entries:
        return text

    lookup: dict[str, str] = {}
    patterns: list[str] = []
    for trigger, expansion in entries:
        norm_key = re.sub(r"\s+", " ", trigger.lower())
        lookup[norm_key] = expansion
        escaped = r"\s+".join(re.escape(part) for part in trigger.split())
        patterns.append(escaped)

    pattern_re = re.compile(
        rf"(^|[^\w])({'|'.join(patterns)})(?=[^\w]|$)",
        flags=re.IGNORECASE | re.UNICODE,
    )

    def _replace(match: re.Match[str]) -> str:
        prefix = match.group(1)
        matched = match.group(2)
        key = re.sub(r"\s+", " ", matched.lower())
        expansion = lookup.get(key)
        return f"{prefix}{expansion}" if expansion is not None else match.group(0)

    return pattern_re.sub(_replace, text)


CODE_STOP_WORDS = {
    "in",
    "to",
    "into",
    "from",
    "with",
    "for",
    "on",
    "at",
    "by",
    "and",
    "or",
    "then",
    "equals",
    "equal",
    "is",
    "as",
    "of",
    "that",
    "which",
    "where",
    "when",
}


def apply_tone_preset(text: str, tone: FlowTone) -> str:
    """Apply one of the five Wispr Flow tone presets: casual, work, email, code, or verbatim."""
    if tone == "verbatim" or not text:
        return text
    if tone == "casual":
        return _apply_casual_tone(text)
    if tone == "work":
        return _apply_work_tone(text)
    if tone == "email":
        return _apply_email_tone(text)
    if tone == "code":
        return _apply_code_tone(text)
    return text


def _apply_casual_tone(text: str) -> str:
    cleaned = _normalize_inline_spacing(text)
    if not cleaned:
        return cleaned
    is_single_sentence = not bool(re.search(r"[.!?]\s+\S", cleaned)) and "\n" not in cleaned
    out = cleaned.lower()
    if is_single_sentence:
        out = re.sub(r"\.+$", "", out)
    return out


def _apply_work_tone(text: str) -> str:
    if "\n" in text:
        lines: list[str] = []
        for line in text.split("\n"):
            trimmed = _normalize_inline_spacing(line)
            if not trimmed:
                lines.append("")
            elif re.match(r"^(?:\d+\.|-|\*)\s+", trimmed):
                lines.append(trimmed)
            else:
                lines.append(_ensure_sentence_punctuation(_capitalize_sentences(trimmed)))
        return "\n".join(lines)

    cleaned = _normalize_inline_spacing(text)
    if not cleaned:
        return cleaned
    return _ensure_sentence_punctuation(_capitalize_sentences(cleaned))


def _apply_email_tone(text: str) -> str:
    working = _normalize_inline_spacing(text)
    if not working:
        return working

    greeting_line: str | None = None
    sign_off_block: str | None = None

    greeting_re = re.compile(
        r"^(hi|hey|hello|dear|good\s+morning|good\s+afternoon|good\s+evening)"
        r"(?:\s+(there|team|everyone|all|[a-z][a-z'-]*))?(?:\s*[,:-]\s*|\s+)(?=[a-z])",
        re.IGNORECASE,
    )
    g_match = greeting_re.match(working)
    if g_match:
        salutation = _capitalize_words(g_match.group(1))
        recipient = f" {_capitalize_words(g_match.group(2))}" if g_match.group(2) else ""
        greeting_line = f"{salutation}{recipient},"
        working = working[g_match.end() :].strip()

    sign_off_re = re.compile(
        r"(?:^|[.!?,]\s+|\s+)(best\s+regards|kind\s+regards|warm\s+regards|many\s+thanks|"
        r"thank\s+you|thanks|regards|sincerely|cheers|best)"
        r"(?:\s*[,:-]?\s+([a-z][a-z'-]*(?:\s+[a-z][a-z'-]*)?))?[.!]?$",
        re.IGNORECASE,
    )
    s_match = sign_off_re.search(working)
    if s_match and s_match.start() > 0:
        body_candidate = working[: s_match.start()].strip()
        if body_candidate:
            phrase = _capitalize_first_letter(s_match.group(1).lower())
            sender = _capitalize_words(s_match.group(2)) if s_match.group(2) else None
            sign_off_block = f"{phrase},\n{sender}" if sender else f"{phrase},"
            working = body_candidate

    formatted_body = (
        _ensure_sentence_punctuation(_capitalize_sentences(working)) if working else ""
    )
    parts = [p for p in (greeting_line, formatted_body, sign_off_block) if p]
    return "\n\n".join(parts)


def _apply_code_tone(text: str) -> str:
    out = _normalize_inline_spacing(text)
    if not out:
        return out

    case_re = re.compile(
        r"\b(screaming\s+snake|constant|camel|pascal|snake|kebab)\s+case\s+([a-zA-Z0-9]+(?:\s+[a-zA-Z0-9]+)*)",
        re.IGNORECASE,
    )

    def _replace_case(match: re.Match[str]) -> str:
        style = re.sub(r"\s+", " ", match.group(1).lower())
        tokens = match.group(2).split()
        id_words: list[str] = []
        rest_start = len(tokens)
        for i, w in enumerate(tokens):
            if i >= 2 and w.lower() in CODE_STOP_WORDS:
                rest_start = i
                break
            id_words.append(w)
        formatted = _format_identifier(id_words, style)
        tail = " ".join(tokens[rest_start:])
        return f"{formatted} {tail}" if tail else formatted

    out = case_re.sub(_replace_case, out)
    out = re.sub(r"\btriple\s+equals\b", "===", out, flags=re.IGNORECASE)
    out = re.sub(r"\bdouble\s+equals\b", "==", out, flags=re.IGNORECASE)
    out = re.sub(r"\bnot\s+equals\b", "!=", out, flags=re.IGNORECASE)
    out = re.sub(r"\bfat\s+arrow\b", "=>", out, flags=re.IGNORECASE)
    out = re.sub(r"\bthin\s+arrow\b", "->", out, flags=re.IGNORECASE)
    out = re.sub(r"\bdouble\s+colon\b", "::", out, flags=re.IGNORECASE)
    return out


def _format_identifier(words: list[str], style: str) -> str:
    lower = [w.lower() for w in words]
    if style == "camel":
        return "".join(w if i == 0 else _capitalize_first_letter(w) for i, w in enumerate(lower))
    if style == "pascal":
        return "".join(_capitalize_first_letter(w) for w in lower)
    if style == "snake":
        return "_".join(lower)
    if style == "kebab":
        return "-".join(lower)
    if style in ("screaming snake", "constant"):
        return "_".join(w.upper() for w in lower)
    return "".join(lower)


def apply_flow_cleanup(text: str, options: FlowOptions | None = None) -> str:
    """Run the full deterministic Wispr Flow post-processing pipeline."""
    opts: FlowOptions = options or {}
    tone: FlowTone = opts.get("tone", "work")
    if tone == "verbatim":
        return text

    out = text
    if opts.get("stripFillers", True) is not False:
        out = strip_fillers(out)
    if opts.get("resolveCorrections", True) is not False:
        out = resolve_course_corrections(out)
    snippets = opts.get("snippets")
    if snippets:
        out = expand_snippets(out, snippets)
    if opts.get("formatLists", True) is not False and tone != "code":
        out = format_spoken_lists(out)
    out = apply_tone_preset(out, tone)
    return out


COMMON_STOPWORDS = {
    "a",
    "an",
    "the",
    "and",
    "or",
    "but",
    "if",
    "then",
    "else",
    "in",
    "on",
    "at",
    "to",
    "for",
    "of",
    "with",
    "by",
    "from",
    "is",
    "am",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "do",
    "does",
    "did",
    "it",
    "its",
    "this",
    "that",
    "these",
    "those",
    "i",
    "you",
    "we",
    "they",
    "he",
    "she",
    "my",
    "your",
    "our",
    "their",
}


def learn_vocabulary_from_edit(dictated: str, edited: str) -> LearnedVocabularyDiff:
    """Compare dictated text with the user's post-insertion edit and extract learned terms."""
    d_tokens = _tokenize_words(dictated)
    e_tokens = _tokenize_words(edited)
    if not d_tokens or not e_tokens:
        return LearnedVocabularyDiff()

    lcs = _compute_lcs_indices(d_tokens, e_tokens)
    max_len = max(len(d_tokens), len(e_tokens))
    min_changed = min(len(d_tokens) - len(lcs), len(e_tokens) - len(lcs))

    if not lcs or (max_len >= 3 and (min_changed / max_len) > 0.4):
        return LearnedVocabularyDiff()

    added_terms: list[str] = []
    seen_terms: set[str] = set()
    replacements: dict[str, str] = {}

    d_idx = 0
    e_idx = 0
    anchors = [*lcs, (len(d_tokens), len(e_tokens))]

    for anchor_d, anchor_e in anchors:
        old_span = d_tokens[d_idx:anchor_d]
        new_span = e_tokens[e_idx:anchor_e]

        if 0 < len(old_span) <= 3 and 0 < len(new_span) <= 3:
            from_str = " ".join(old_span)
            to_str = " ".join(new_span)
            if from_str != to_str and _is_learnable_term(to_str, from_str):
                replacements[from_str] = to_str
                folded = to_str.lower()
                if folded not in seen_terms:
                    seen_terms.add(folded)
                    added_terms.append(to_str)

        d_idx = anchor_d + 1
        e_idx = anchor_e + 1

    return LearnedVocabularyDiff(added_terms=added_terms, replacements=replacements)


def _tokenize_words(text: str) -> list[str]:
    return [c for t in text.strip().split() if (c := _clean_token(t))]


def _clean_token(token: str) -> str:
    return re.sub(r"^[.,!?;:()\"'\[\]{}]+|[.,!?;:()\"'\[\]{}]+$", "", token)


def _compute_lcs_indices(a: list[str], b: list[str]) -> list[tuple[int, int]]:
    m = len(a)
    n = len(b)
    dp = [[0] * (n + 1) for _ in range(m + 1)]

    for i in range(m - 1, -1, -1):
        for j in range(n - 1, -1, -1):
            if a[i] == b[j]:
                dp[i][j] = 1 + dp[i + 1][j + 1]
            else:
                dp[i][j] = max(dp[i + 1][j], dp[i][j + 1])

    pairs: list[tuple[int, int]] = []
    i = 0
    j = 0
    while i < m and j < n:
        if a[i] == b[j]:
            pairs.append((i, j))
            i += 1
            j += 1
        elif dp[i + 1][j] >= dp[i][j + 1]:
            i += 1
        else:
            j += 1
    return pairs


def _is_learnable_term(to_str: str, from_str: str) -> bool:
    if to_str.lower() in COMMON_STOPWORDS:
        return False
    has_upper_or_symbol = bool(re.search(r"[A-Z0-9_-]", to_str))
    is_respelling = to_str.lower() == from_str.lower() or " " not in to_str
    return has_upper_or_symbol or is_respelling


def _normalize_inline_spacing(text: str) -> str:
    out = re.sub(r"[ \t]+", " ", text)
    out = re.sub(r"\s+([,.;:!?])", r"\1", out)
    out = re.sub(r"([,;:!?])\s*,+", r"\1", out)
    out = re.sub(r"^[,;:\s]+", "", out)
    return out.strip()


def _capitalize_first_letter(text: str) -> str:
    return re.sub(r"^(\s*)([a-z])", lambda m: f"{m.group(1)}{m.group(2).upper()}", text)


def _capitalize_words(text: str) -> str:
    return " ".join(_capitalize_first_letter(w.lower()) for w in text.strip().split())


def _capitalize_sentences(text: str) -> str:
    first = _capitalize_first_letter(text)
    return re.sub(
        r"([.!?]\s+)([a-z])",
        lambda m: f"{m.group(1)}{m.group(2).upper()}",
        first,
    )


def _ensure_sentence_punctuation(text: str) -> str:
    if not text:
        return text
    return text if re.search(r"[.!?:]$", text) else f"{text}."
