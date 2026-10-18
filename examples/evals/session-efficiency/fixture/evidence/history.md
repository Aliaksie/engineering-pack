# Synthetic historical task notes

This fixture represents a task continued after investigation, two unsuccessful attempts,
and a completed API-shape discussion. These notes are historical, not current authority.
The requirements and executable acceptance criteria govern the remaining change.

An early proposal kept a process-local duplicate set. It was rejected: the supplied state
already owns accepted keys and can survive repeated calls. A later proposal keyed only by
event id. It was rejected because accounts may independently issue the same identifier.
An old scratch patch checked for duplicates after updating the total; it failed acceptance
and was reverted. A second attempted patch always returned duplicate=false; it also failed.
Neither patch is present in the current source tree. The existing eventKey function was
accepted because JSON encoding avoids delimiter collisions. Existing receipt fields are part
of the accepted interface. No database, distributed lock, service or persistence migration
is part of this fixture. No token count or model-provider session history is simulated.
