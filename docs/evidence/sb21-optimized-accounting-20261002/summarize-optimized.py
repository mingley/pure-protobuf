import json
from pathlib import Path

root = Path('work/sb21/optimized')
rows = []
for p in sorted(root.rglob('*-accounting.json')):
    proof = json.loads(p.read_text())
    result = json.loads(p.with_name(p.name.replace('-accounting.json', '-result.json')).read_text())
    m = proof['measurement']
    row = {'artifact': str(p.relative_to(root)), 'accounting_verified': proof['accounting_verified'],
           'claim_eligible': proof['claim_eligible'], 'window_seconds': m['window_seconds'],
           'completion_qps': proof['completion_qps']}
    for key in ('offered', 'dispatched', 'completed', 'successful', 'failed', 'rejected',
                'timed_out', 'incoming_in_flight', 'carried_in_completed', 'unfinished'):
        row[key] = m[key]
    for endpoint in ('client', 'server'):
        stats = result[endpoint + 'Stats'][0]
        row[endpoint + '_cpu_pct'] = 100 * (stats.get('timeUser', 0) + stats.get('timeSystem', 0)) / stats['timeElapsed']
    for kind in ('service', 'scheduled'):
        hist = m[kind + '_latency_nanos']
        row[kind + '_latency_mean_nanos'] = hist['sum'] / hist['count'] if hist['count'] else None
    rows.append(row)
summary = {'source_pins': json.loads((root / 'pins.json').read_text()),
           'lease': json.loads((root / 'lease.json').read_text()), 'rows': rows,
           'claim_eligible': False,
           'limits': ['1/2-second loopback diagnostic on a shared host; no contract qualification.',
                      'Native release and Go optimized binaries; exact unchanged aggregate rate 5000.',
                      'Old stack sampler denominator remains unverified and is not used as headroom proof.',
                      'Official endpoint CPU percentages retain their own driver mark windows; capacity proof is absent.']}
(root / 'diagnostic-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
for row in rows:
    print(row['artifact'], 'PASS' if row['accounting_verified'] else 'FAIL',
          'offered', row['offered'], 'completed', row['completed'], 'reject', row['rejected'],
          'unfinished', row['unfinished'], 'qps', round(row['completion_qps'],1),
          'CPU client/server', round(row['client_cpu_pct'],1), round(row['server_cpu_pct'],1))
