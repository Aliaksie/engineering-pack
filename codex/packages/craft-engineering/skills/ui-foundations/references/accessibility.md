# Make the interaction operable

Begin with semantic elements and a logical document structure. Give controls accessible names,
associate inputs with labels and errors, and expose actual state such as expanded or selected.
Use ARIA when native semantics cannot represent the interaction, following the consumer's
supported widget pattern.

Exercise the complete task with a keyboard. Check visible focus, sensible order, escape behavior
and focus restoration after transient UI closes. A modal needs an intentional focus lifecycle;
changing content should not leave focus in a removed subtree. Announce important asynchronous
outcomes without making every small refresh interruptive.

Example: opening a filter dialog moves focus to its meaningful entry point; closing returns it
to the invoking control unless navigation made that control irrelevant. A failed apply action
keeps the dialog usable and exposes its error.

Check zoom, reflow, contrast, reduced-motion preferences and alternative text where relevant.
Automated checks detect some structural problems, but names, reading order and task usability need
human or interaction evidence. Report the actual checks instead of claiming full accessibility
from a single automated score.
