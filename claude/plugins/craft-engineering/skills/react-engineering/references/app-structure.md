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
