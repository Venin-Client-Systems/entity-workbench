"""Fail closed: a build success cannot stand in for release acceptance evidence."""
from pathlib import Path
import json,sys
root=Path(__file__).resolve().parents[1]
gates=json.loads((root/'docs/release-gates.json').read_text())
required={'integrated_workflows','windows_appcontainer','macos_signed_helpers','all_runtime_dependencies_bundled','offline_clean_install_three_targets','hostile_input_and_resource_exhaustion','broad_local_web_coverage','recovery_and_migration_failures','benchmark_16gb_target','dependency_license_and_security_review','signed_and_notarized_artifacts','downloaded_artifact_verification'}
assert gates['schema_version']==1 and set(gates['gates'])==required
assert all(type(value) is bool for value in gates['gates'].values())
missing=sorted(name for name,passed in gates['gates'].items() if not passed)
assert gates['complete_release']==(not missing),'Release claim conflicts with evidence gates'
print(json.dumps({'complete_release':not missing,'unpassed':missing},indent=2))
if '--check-policy' not in sys.argv and missing:raise SystemExit(1)
