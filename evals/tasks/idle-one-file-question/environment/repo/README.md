# Notifier

Sends order notifications to customers by email and SMS.

- `src/messages.js` renders the customer-facing texts.
- `src/retry.js` retries a failed delivery with a growing delay.
- `src/send.js` picks a channel and delivers one notification.

Run the tests with `npm test`.
