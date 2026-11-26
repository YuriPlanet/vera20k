// Fixed one-second window shared by the relay transport and LAN signaling.
export function rateLimit(maximum) {
  let start = performance.now();
  let count = 0;
  return () => {
    const now = performance.now();
    if (now - start >= 1000) { start = now; count = 0; }
    return ++count <= maximum;
  };
}

// One aggregate budget of `total` per second, shared max-min fairly by its
// members, each spending at most `maximum`. Every window reserves for each
// member what it attempted in the previous window, up to an equal share of
// what smaller members leave (water-filling), so steady traffic cannot be
// spent by others. Packets past a reservation take only unreserved budget;
// a member that joins mid-window reserves nothing until the next one.
export function fairBudget(total, maximum) {
  const members = new Set();
  let start = performance.now();
  let spent = 0;
  // Reserved packets not yet spent; spent + reserved never exceeds total.
  let reserved = 0;
  const roll = () => {
    const now = performance.now();
    if (now - start < 1000) return;
    start = now;
    spent = 0;
    let left = total;
    const ascending = [...members].sort((a, b) => a.demand - b.demand);
    ascending.forEach((member, index) => {
      member.reserve = Math.min(member.demand, maximum, Math.floor(left / (ascending.length - index)));
      left -= member.reserve;
      member.demand = 0;
      member.used = 0;
    });
    reserved = total - left;
  };
  return {
    join() {
      const member = { demand: 0, used: 0, reserve: 0 };
      members.add(member);
      return {
        take() {
          roll();
          member.demand += 1;
          if (member.used >= maximum) return false;
          if (member.used < member.reserve) reserved -= 1;
          else if (spent + reserved >= total) return false;
          member.used += 1;
          spent += 1;
          return true;
        },
        leave() {
          if (members.delete(member)) reserved -= Math.max(0, member.reserve - member.used);
        },
      };
    },
  };
}
