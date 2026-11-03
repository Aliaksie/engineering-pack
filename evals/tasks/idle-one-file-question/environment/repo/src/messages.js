// Customer-facing notification texts.
export function orderConfirmed(order) {
  return `Thanks, ${order.customer}! Your order ${order.id} was recieved and is being prepared.`;
}

export function orderShipped(order) {
  return `Good news: order ${order.id} is on its way. Track it at ${order.trackingUrl}.`;
}

export function orderDelayed(order, days) {
  const unit = days === 1 ? 'day' : 'days';
  return `Sorry, order ${order.id} is running ${days} ${unit} late.`;
}
