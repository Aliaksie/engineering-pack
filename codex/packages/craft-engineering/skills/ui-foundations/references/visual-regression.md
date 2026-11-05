# Use rendered differences as evidence

Specify the visual contract and capture representative states under comparable conditions.
Pin viewport, fonts, data, theme and motion/time inputs where practical. Use synthetic content
that exercises long labels, overflow and disabled/error states without exposing private data.

Compare baseline and candidate images for the intended change and unintended shifts. A pixel
difference is a signal to inspect; antialiasing noise is not automatically a product defect and
a large accepted diff can conceal a genuine regression. Review cropping, missing resources and
layout stability before trusting the capture.

Example: compare an expanded details panel at narrow and wide widths with a long reference
number. Inspect wrapping, action visibility and the surrounding page flow, then verify the panel
remains keyboard-operable.

Do not update baselines simply to pass CI. Record why a change is accepted and retain useful
evidence. Screenshots supplement semantic/interaction checks; they cannot prove server permissions,
accessible names or behavior in states that were never rendered.
