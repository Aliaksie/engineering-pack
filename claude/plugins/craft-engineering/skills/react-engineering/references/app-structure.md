# Organize the interface around change

Map the user interaction through routing, data access and rendered components. Locate the owner
of remote data, application policy and presentation state. Follow the consumer's established
boundaries unless a concrete dependency or ownership problem requires a change.

Keep network serialization and validation at an identifiable boundary. Components should receive
meaningful results rather than repeatedly decoding transport details. Do not place server secrets
or trusted authorization in a client module. Confirm server/client rendering boundaries before
moving imports or asynchronous work.

Split by a coherent responsibility, not a target file size. A reusable visual primitive and a
product-specific flow may deserve different homes; a wrapper forwarding every prop may add no
useful boundary. Check package exports and public component contracts before restructuring.

Example: a shipment-status page can own routing and query selection while a status timeline
renders already interpreted events. Test the mapping separately from keyboard interaction.
Verify loading, denied access and navigation changes through the actual routing/data boundary.

## Locate the performance constraint

For a slow interaction, distinguish network waiting, JavaScript transfer/parse cost and React
render work before changing structure. Use a network trace, bundle report or
[React Profiler](https://react.dev/reference/react/Profiler) for the affected boundary.
Start independent reads together when permissions, resource limits and failure handling allow;
retain ordering where one result authorizes or supplies another operation. Bound fan-out over
large collections rather than starting every request at once.

Defer expensive optional UI with the project's supported loading mechanism when it improves
the measured interaction. Preserve loading/error behavior and avoid introducing a new request
waterfall. Use documented package exports and check the actual bundle before replacing imports;
a shorter import path does not prove a smaller bundle. Next.js-specific loaders, caches and
server APIs require that framework and a supported version; React alone does not supply them.
