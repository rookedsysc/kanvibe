const response = http.get(`${PROBE_API_FROM_HOST}/state?task=${TASK}&marker=${MARKER}&timeoutMs=15000`);
if (response.status !== 200) throw new Error(`Transport probe failed: ${response.status}`);
const state = json(response.body);
const inputs = state.tasks[TASK].input;
const occurrences = inputs.split(MARKER).length - 1;
if (occurrences !== 1) throw new Error(`${TASK} received marker ${occurrences} times`);
if (TASK === 'direct' && state.counts.direct.relayAttaches !== 0) throw new Error('Direct task used A relay');
if (TASK === 'relay' && (state.counts.relay.relayAttaches < 1 || state.counts.relay.relayInputs < 1)) throw new Error('Relay task did not use persistent B transport');
output.transportVerified = true;
