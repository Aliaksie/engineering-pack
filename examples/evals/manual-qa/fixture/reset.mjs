// Deliberately faulty local application: the token is never consumed.
// No email, filesystem, customer data or network effects.
export function resetApplication() {
  let changes = 0;
  return {
    redeem(token) {
      if (token !== 'synthetic-token') return { accepted: false, changes };
      changes += 1;
      return { accepted: true, changes };
    }
  };
}
