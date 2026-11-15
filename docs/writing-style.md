# Writing style

Write all text in this repository in the style of ASD-STE100 Simplified Technical
English (STE). This file gives the rules. It does not claim formal STE compliance:
the approved STE dictionary is copyright ASD, and a formal claim needs a check
against the licensed specification.

## Priority

Technical accuracy and safety come first. Never remove a qualifier, a boundary
condition or a safety warning to make a sentence shorter.

## Where the rules apply

| Text | Level |
|---|---|
| Documentation, READMEs, runbooks, procedures, release notes, error messages, UI text | All rules |
| Commit messages, PR text, code comments, specs, plans, reports, issues | All rules. Domain words are permitted. |
| Chat replies | Short sentences, active voice, one idea in each sentence, no idiom |

Do not change source code, commands, identifiers, file paths, API names, log
output, quotations, legal text or safety text.

## Rules

1. Use common words. Use one word for one meaning. Do not use a different word
   for the same thing.
2. Use the short word: "use", not "utilize"; "start", not "initiate"; "about",
   not "approximately"; "make sure", not "ensure".
3. Use the active voice. Use the imperative for each instruction. Do not use
   contractions.
4. Do not use slang, idiom, metaphor, humor or marketing words.
5. Do not use a pronoun when the reader cannot tell what it refers to. Do not use
   filler words such as "very", "simply", "just" or "seamless".
6. Define each abbreviation and each technical term at its first use. Then use the
   same term each time.
7. Use only these verb forms: infinitive, imperative, simple present, simple past,
   simple future, and the past participle as an adjective. Do not use the -ing
   form, except in a technical name. Do not use the perfect or progressive tenses.
8. Keep a procedure sentence to 20 words or fewer. Keep a descriptive sentence to
   25 words or fewer when you can. Keep a paragraph to 6 sentences. Use no more
   than 3 nouns in a row.
9. Put one instruction or one idea in each sentence. Use a numbered list for two
   or more steps. Use a table to compare items.
10. Put the condition first: "If the test fails, start the server again."
11. Put a warning or a caution before the step that it applies to.
