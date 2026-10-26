# Verify what users can do

Select the boundary that owns the behavior. Pure mappings may need a unit check; component tests
can verify names, state and interactions; routing, browser APIs and server contracts may require
integration or browser evidence. Use the actual project runner and supported rendering setup.

Query by meaningful roles and names when available. Interact through user-visible controls and
assert the resulting outcome, not internal hook order. Wait for asynchronous state deliberately;
arbitrary sleeps hide races and slow the suite. Mock at a useful external boundary and disclose
what the mock cannot establish.

Example: edit a delivery note, force a rejected save, then verify the text remains editable and
the error is associated with the form. A test that only checks the submit callback fired does
not cover recovery.

Include rapid state changes, cleanup and denied/empty/error paths when affected. Browser evidence
is necessary for actual focus, layout, scrolling and platform behavior that a DOM simulator does
not implement. Avoid snapshots that accept large unrelated changes without a clear contract.
