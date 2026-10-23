# prose.md

How documents are written here. The mechanical half is checked by a linter; the
rest is a reading.

---

## The rule is a register, and it has a name

**Write every document here in ASD-STE100, Simplified Technical English.**
Not a house style, not a list of banned characters: a controlled language,
published and maintained outside this repository, written so that a procedure
means one thing to every reader, including one reading it in a second language
and one reading it under pressure.

It binds every document in this tree, every document a project inherits from
it, and every commit message, handoff, record entry and review write-up. There
is no register for the important pages and a looser one for the rest.

Beyond the register: every claim is backed by a command a reader can run or a
path a reader can open. Write for an agent with no memory of the session that
wrote the file, and for a person who is looking for one fact.

### The rules, in full, in this repository's words

**These are the working rules. They are not a summary of the standard and
they are not optional.** The standard itself is somebody else's document and is
not reproduced here; what follows is the subset that governs writing in this
tree, stated so that a reviewer can hold it and an author can check themselves
against it.

**Words**

1. **One word, one meaning, one part of speech.** A term keeps the same sense
   for the whole document. A check refuses, a guard refuses, a gate refuses:
   pick one and keep it.
2. **Never reach for a synonym to avoid repetition.** Repeating the term is
   correct. Varying it makes a reader ask whether you mean something else, and
   in a technical document they are right to ask.
3. **Do not use a noun as a verb, or a verb as a noun.** "Action the finding"
   and "the ask" are both refused.
4. **Use the real noun rather than one made from a verb.** "It fails" beats "a
   failure occurs". "The check refuses it" beats "refusal takes place".
5. **No more than three nouns in a row.** "Gate check exit code handling" has
   four and means nothing. Break it with a preposition: "handling the exit code
   of a gate check".

**Verbs and voice**

6. **Active voice.** "The check refuses it", never "it is refused".
7. **Simple tenses only.** Present for what is true, past for what happened.
   Not "will have been", not "would have needed to be".
8. **Do not drop the verb.** A heading may; a sentence may not.
9. **Start an instruction with its verb.** "Run the gate before you push", not
   "The gate should be run before pushing".

**Sentences and paragraphs**

10. **One instruction per sentence.** A sentence carrying a second clause
    between dashes is two instructions wearing one.
11. **An instruction is at most 20 words. A descriptive sentence is at most
    25.** This one is a reading rather than a check, and the measurement is
    below.
12. **A paragraph covers one topic and runs to at most six sentences.**
13. **Put the condition before the instruction.** "If the check exits 2, read
    the first line" rather than "read the first line if the check exits 2". A
    reader who acts on the first half of a sentence must not have acted wrongly.
14. **Put a warning before the step it applies to**, never after. A caution a
    reader meets afterwards is a caution about damage already done.

**Punctuation**

15. **A sentence that needs a long dash is a sentence to split**, and every
    spelling of the dash is the same defect. The section above this one carries
    the rule and what it cost.
16. **No slash meaning "and or".** Write which one you mean, or write both.
17. **Nothing essential inside parentheses.** A reader skips them. If it
    matters, it is a sentence.

**What this cannot hold**

**Rules 1 to 5 and 16 to 17 are readings.** No linter can decide whether two
words mean the same thing, and one that tried would refuse correct writing.
**Rule 15 is checked**, because a respelt dash is a character sequence rather
than a judgement.

**Rule 11 is a reading here, and the number is why.** Measured over this
tree's markdown prose on 2026-09-18, on one Windows 11 Pro 10.0.26200 machine:
**about 15% of sentences run past 25 words**.

**The ratio is quoted and the raw counts are not.** The tree grew while the
measurement was being written up, so a count taken before publishing is wrong
by the time anybody re-runs it. The ratio held across that growth.

**Treat even the ratio as an upper bound.** The instrument splits on
sentence punctuation, so a bulleted list whose items carry no full stop is
measured as one long sentence, and every one of the longest results it reported
was that shape rather than a real sentence. **Arming a ceiling on
an instrument that cannot tell a list from a sentence would refuse correct
writing on its first run**, which is the failure mode that gets a check switched
off within a week. The rule stays, the number stays with its conditions, and a
ceiling waits for a splitter that can tell the difference.

**Most of STE is a reading, and a linter declines it on purpose.** A guard
over prose either passes vacuously or refuses legitimate writing. The sections
below are the parts a machine can hold, and holding them is not the same as
writing well. [`check-docs.sh`](../../scripts/common/check-docs.sh) says so in
its own header rather than leaving the next session to rediscover it, and
the register is one of the three review lenses instead:
[`../methodology/reviews.md`](../methodology/reviews.md).

---

## The dash rule is about the sentence, not about the character

**A sentence that needs a dash is a sentence to split.** That is the whole
rule, and every spelling of the dash is the same defect.

**Banning the character taught agents to respell it.** Reported by the
template's operator on 2026-09-17: agents reaching for an em dash, finding
the character refused, and writing `--` or a comma instead. Every run stayed
green. Not one sentence got shorter. That is the same shape as the marker
story further down this page, where keeping strictly to five allowed
characters was read as compliance and nothing measured the thing the rule
existed for.

So the rule now names the construction, and the check follows it as far as a
check can go:

| spelling | what happens |
| --- | --- |
| the em dash character | refused by the character rule below, which permits only ASCII |
| a run of hyphens, spaced or between two words | refused by [`check-markers.sh`](../../scripts/common/check-markers.sh), in markdown prose |
| a comma splice carrying the same parenthetical | **not checkable, and the commonest of the three.** A comma is an ordinary character, and no guard can tell a parenthetical from a list. |

**The third row is why a green run is not a well-written page.** Split the
sentence, or make the clause its own sentence, or put it in a table. Respelling
the dash changes nothing a reader experiences.

**A spaced single hyphen is deliberately not refused.** No incident in the
template involved one, and measured over the template on 2026-09-18 no
markdown line in prose used one. A rule with no incident behind it is a
preference.

**Four exemptions, each because the text is not this repository writing.**
A fenced block and a code span, so a page can show the spelling it refuses. A
quoted line, because superseded wording is kept verbatim by a rule of its own.
A table row and a heading, because neither is a sentence. A link target and a
bare URL, because a hyphen pair inside a path somebody else owns is not prose.

---

## No emoji, and no marker characters

**Write no emoji and no marker character, in any file.** That covers
documents, scripts, workflow files and commit messages. It covers status
output too.

**Use bold text for emphasis, and a plain-text label to flag a trap.** Three
labels are allowed, and each means one thing:

| label | use it for |
| --- | --- |
| `WARNING:` | an action that can cause a loss you cannot undo, or a rule that was already broken once |
| `CAUTION:` | a trap. It works until it does not, and the failure is quiet. |
| `NOTE:` | information that helps the reader, but is not a rule |

**Put the label at the start of the sentence it applies to.** Use one label or
none. Do not stack labels. Do not make a label stronger with capitals or with
repetition. A page with three levels of stop makes a reader weigh them, and a
label exists to prevent that.

**Use labels sparingly enough that they are still visible.** A page where
every paragraph carries one has no labels at all. If a page needs more than a
handful, the page is a rulebook pretending to be a summary. Split it.

**Report a status as a word.** Machine output, result tables and checklists
write `ok`, `FAIL`, `SKIP`, `pass` or `fail`. A word can be searched for, and
every terminal can print it.

### Why there are none

The template this project came from allowed five marker characters: three for
prose and two status glyphs. **An agent kept strictly to the five allowed
characters and used them until the documents were unreadable.** Keeping to the
allowlist was treated as compliance, and nothing said how many. The template
then measured the density of its markers and set a ceiling.

This project allows none. A rule of zero needs no ceiling, no allowlist and no
argument about one more glyph. The template keeps the retired wording and its
measurements.

### What the check holds

[`check-markers.sh`](../../scripts/common/check-markers.sh) holds the
character rules over **every tracked text file**, not only markdown:

1. Every text file is ASCII.
2. No emoji and no marker character, anywhere. The script header lists the
   code point ranges it refuses.
3. No em dash respelt in hyphens, in markdown prose. The section above carries
   that rule.
4. No unclosed fence in markdown, because an open fence switches the other
   rules off.

**A specimen inside a code span or a fenced block may carry a non-ASCII
character**, so a page can show a reader which character it means. **That
exemption does not cover an emoji or a marker character.** Name one by its
code point instead, for example U+26D4.

**The check reads every tracked text file because the markdown-only version
missed every script.** On the day the template widened it, the template's own
scripts held **2290** characters outside its allowed set, across 22 files. A
rule that only looked at documents had left every script unchecked.

**The linter owns the character set. It does not own the labels.** Nothing
mechanical can tell whether a `WARNING:` marks a real loss or a preference. A
label used wrongly passes the check and fails the review.

---

## Amend in place. Do not stack banners.

**When a rule changes, rewrite the rule.** Do not append a dated box under
the old text saying the text above is retired.

This is the correction with the most evidence behind it. A document written by
accretion, where the paragraph says one thing and a box below it says the
opposite, has a documented failure mode: an agent reads the first paragraph of
the box, stops, and acts on the retired rule. It happened, it broke a rule
about publishing, and the incident report is the reason this section exists.

What to do instead:

1. **Rewrite the rule to what it is now.** The current text is the only text.
2. **Move the superseded wording to the history directory**, with the date and
   why it changed. A separate file, not a box on the live page.
   [`../methodology/history.md`](../methodology/history.md) says where that
   is and what the directory's own rules are. It exists because this
   instruction used to say "a history file" without saying where, so the
   superseded wording went into the page that was superseding it, and a whole
   document set filled up with narrative.
3. **Link to it once**, from the rule, in a sentence.

The story of a change belongs in the changelog or the history file. The
document says what is true now. A reader reaching for a rule needs one answer,
and a page that offers two makes them guess which is live.

This is not licence to delete. A superseded rule is moved, never dropped, so
a future session that wonders why the rule is what it is can find out instead of
re-deriving it wrongly.

---

## Say what is not true

Reserve an explicit place for the truths that are tempting to hide. This is
slower than it looks. This feature has a known gap. This estimate excludes
something that cannot be measured.

**Never a fabricated number.** When the real value is unknown, write a dash.
A wrong number on a report is worse than no number, because a blank gets
checked and a number gets used.

**A measurement carries its conditions or it is not a measurement.** A rate
with no date, no machine, no sample count and no input size cannot be compared
to anything, which makes it worse than an absence: it invites a comparison that
means nothing.

---

## Every claim is verified before it is written

Writing the documentation is the audit. Being forced to state precisely what
something does, and then checking whether that is true, is where a startling
share of real defects are found. Expect the documentation pass to generate
findings, and treat that as the feature rather than as a delay.

Do not copy a number out of another document. Derive it, or name where it
came from. A value in two places with no check between them drifts, and the
copy a reader trusts is the wrong one.

---

## One fact, one home

Every fact lives in exactly one document. If it must appear in a second place,
derive it there or have a check assert that the two agree.

**This is checked now, and it had drifted badly while it was only a
sentence.** [`check-one-home.sh`](../../scripts/common/check-one-home.sh)
refuses a sentence of 12 words or more that appears in two documents. Its first
run over the template found **42** duplicates, five of them in the skeleton
the template ships for recording a project's rules.

**Two exemptions, both narrow.** The entry-point routers are exempt from each
other, because each states the absolutes in full for a session that may be
handed exactly one of them; a router sharing a sentence with anything else is
still refused. And the history directory is exempt entirely, because a
superseded page states things the live pages now state differently, which is
the point of it. [`../methodology/history.md`](../methodology/history.md).

When two documents conflict, the technical reference wins and the other one is
the defect. Fix it in the same change and say so.

---

## What a document is not

**A document says what the thing does. It does not say what the project did.**

A fixed defect belongs in a reference page only when a reader needs it to use
the thing correctly. "The allocator takes a write lock now" is history and
belongs in the work record. "Two lints exist because another client will refuse
the file" is a constraint and stays.

**The history goes to the history directory, and there is one.**
[`../methodology/history.md`](../methodology/history.md). An agent working from
this template wrote its project's history into every document it touched;
nothing it wrote was untrue and the result was unreadable. The instinct is
right, which is why forbidding it does not work: a superseded explanation is
often the only record of why a design has its shape. It needed a destination,
not a prohibition.

An unlinked page is not read, so it is not corrected, and that is the state
every stale document passes through on the way to being wrong. A page nothing
links to is a finding.

---

## The mechanical half, which a linter checks

A documentation linter catches the things that rot silently and that no other
check sees:

1. **Every fenced shell block parses.** A block that does not parse is a block
   nobody can copy and paste.
2. **No angle-bracket placeholders inside a shell block.** A human reads
   `<deployment-id>` as "fill this in" and bash reads it as a redirect, so the
   reader gets a cryptic syntax error instead of an obvious instruction. Use an
   upper-case name or a quoted variable.
3. **No literal control bytes.** Documentation about escape sequences has a
   proven habit of containing the character it is warning about.
4. **Every relative link resolves**, and every cited path exists.
5. **No page under the docs directory that nothing links to.**

**The character rules are checked by a different script, over a wider
scope.** The ASCII rule, the no-emoji rule and the respelt dash belong to
[`check-markers.sh`](../../scripts/common/check-markers.sh), which reads every
tracked text file rather than the documents alone.
[`../../scripts/common/README.md`](../../scripts/common/README.md) says why one
rule gets one enforcer: two checks holding one rule is two places for it to be
wrong, and these two would have been wrong differently.

**What a linter cannot check is whether a claim is true.** That is a reading,
and it belongs to the review pass. A guard that tried to verify prose would
either pass vacuously or refuse legitimate writing, and both are worse than an
honest scope.

---

## Where the register actually goes wrong

The standard is at the top of this page. This section is what measuring it
against this tree found, which is not what anybody guessed.

**The failure mode here is metaphor used as jargon, not marketing copy.**
This is worth stating precisely, because the obvious guess is wrong and the
repository proved it wrong: a list of quality-asserting adjectives was written
into this page and never enforced. Armed on 2026-09-10 over this tree it found
**one** hit, `bulletproof`, in a table cell. The same tree writes
**`load-bearing` twelve times, across ten documents**, as if it were a technical
term. That is the register that actually goes wrong, and no word list holds
it: the offending words are ordinary until they are asked to carry a meaning
they do not have.

**The two substitutions that do most of the work:**

- **Replace the adjective with the measurement.** "Fast" becomes the number and
  its conditions. "Robust" becomes what it survives.
- **Replace the metaphor with the mechanism.** "Load-bearing" becomes what
  breaks when it moves. "Surgical" becomes what it touches and what it does
  not.

**`simply`, `just`, `obviously` and `of course` are the words to watch
hardest**, and they are ordinary English, which is why no check can have them.
They tell a reader who is stuck that the thing they cannot do is easy. This
tree uses `just` twenty-two times and almost every one of them means "only",
which is exactly why matching the word would have been useless.

---

## Defensive framing is not neutral

**Describe what code does in plain technical terms.** Do not write up-front
disclaimers arguing that something is legitimate, and do not tell a future
reader not to re-open a question.

Both backfire, in opposite directions. A defensive paragraph primes a skeptical
reader to look for the thing it denies. And a grepping agent trips on the
reassurance words themselves and spends its budget reading the matched line.

State the mechanism and its constraints. Name prior art briefly if it helps.
Stop there. The same applies to identifiers: prefer a neutral accurate name to
an evocative one.
