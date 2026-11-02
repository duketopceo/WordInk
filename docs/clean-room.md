# Clean-room policy: TypeWhisper

TypeWhisper is licensed GPL-3.0. WordInk is MIT. To keep it that way,
TypeWhisper is used for ideas only. No code, strings, assets, identifiers,
file layouts or translations are copied from it, in whole or in part.

## What counts as an idea

A feature at the level a user would describe it, such as "dictation modes that
rewrite text for the app you are typing into". Behavior you can observe by
using the product is an idea. Its source, text and artwork are not.

## How an inspired feature is written

1. **Write the requirement first.** State the feature in WordInk's own words:
   what the user does, what they see, and the edge cases. Put it in the plan or
   the issue, without quoting TypeWhisper.
2. **Do not have its source open.** Whoever writes the code must not read or
   have open TypeWhisper's source while implementing. Use only the written
   requirement.
3. **Design from WordInk's architecture.** Use WordInk's own types, names,
   module layout and strings. Write UI copy and icons fresh.
4. **No translation by rewording.** Re-typing or paraphrasing its code line by
   line is copying. If a solution looks the same because it must (a standard
   algorithm or an API call), it came from the public spec or documentation,
   not from TypeWhisper.
5. **Record it.** In the PR description, say the feature was inspired by an
   idea, link the requirement, and confirm that no GPL source was copied.

## If something slipped in

Remove the code, rewrite it from the requirement by someone who has not seen
the original, and rewrite the history if it reached a release. Rewriting
history does not remove files from a published GitHub release: if copied
material reached one, delete that release and publish a replacement built from
clean code (use a new tag if immutable releases are enabled). Report it to the
maintainer through GitHub private reporting.
