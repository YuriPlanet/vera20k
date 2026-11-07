// Relay grants for the spawn mock, in the lobby's format (docs/lobby-protocol.md:
// RelayGrant, V2 relay tickets). The relay checks them with
// services/relay-protocol's Ticket::sign (src/lib.rs) and OrderingCertificate::sign
// (src/sequenced.rs). It is a test mock, never a deployment.
import { createHmac } from 'node:crypto';

export const base64url = (bytes) => Buffer.from(bytes).toString('base64url');

const uuidBytes = (uuid) => Buffer.from(uuid.replaceAll('-', ''), 'hex');

export function relayGrant(keyHex, address, matchId, sourceIndex, now, playerId, generation, payloadKey, lifetime, ordering) {
  const sign = (domain, bytes) => createHmac('sha256', Buffer.from(keyHex, 'hex')).update(domain).update(bytes).digest();
  const ticket = Buffer.alloc(98);
  ticket[0] = 2;
  uuidBytes(matchId).copy(ticket, 1);
  ticket[17] = sourceIndex;
  ticket.writeBigUInt64LE(BigInt(now + lifetime), 18);
  crypto.getRandomValues(ticket.subarray(26, 42));
  uuidBytes(playerId).copy(ticket, 42);
  ticket.writeBigUInt64LE(BigInt(generation), 58);
  sign('VERA20k relay ticket v2\0', ticket.subarray(0, 66)).copy(ticket, 66);
  // The clock capability outlives loading and renewals; each control still
  // needs its own live ticket. ordering.pacing 'on' or 'off' issues VO2C with
  // the host's pacing flag; without it, VO1C.
  const v2 = ordering.pacing === 'on' || ordering.pacing === 'off';
  const signed = v2 ? 35 : 34;
  const certificate = Buffer.alloc(signed + 32);
  certificate.write(v2 ? 'VO2C' : 'VO1C');
  ticket.copy(certificate, 4, 1, 17);
  certificate.writeUInt32LE(ordering.sources_mask, 20);
  certificate.writeUInt16LE(ordering.tick_hz, 24);
  certificate.writeBigUInt64LE(BigInt(now + 86400), 26);
  if (v2) certificate[34] = ordering.pacing === 'off' ? 1 : 0;
  sign(`VERA20k relay ordering certificate ${v2 ? 'v2' : 'v1'}\0`, certificate.subarray(0, signed))
    .copy(certificate, signed);
  return { address, ticket: base64url(ticket), packet_key: base64url(sign('VERA20k relay packet key v2\0', ticket)),
    payload_key: payloadKey, source_index: sourceIndex, ordering_certificate: base64url(certificate) };
}
