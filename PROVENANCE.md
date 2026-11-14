# Snapshot provenance

Origin: private khalilhimura/nous at 5df00f37af90057b7fbd135732c33d32b428cc2e.

The import manifest binds source Git blobs, SHA-256 digests and file modes to this snapshot. Original commit IDs in historical evidence are origin references, not ancestors here. Historical packets and frozen fixtures are unchanged. Local vault, working-store, environment and target directories and Nous application workspaces are outside the allowlist. This is not a content-clearance or credential-absence attestation.

Some historical documentation links target excluded Nous documents. Resolve those as origin references in the private Nous repository at the source commit above; their targets are not claimed to exist in this standalone tree. No private Git history is imported.

The repository owner approved public publication of the reviewed import with an MIT license on 14 November 2026. LICENSE supplies the grant; NOTICE preserves attribution and third-party rights. The original source history remains in Nous. The source crate version and all runtime source, profiles, evals and frozen fixture bytes are preserved; the manifest identifies test/tooling/documentation adaptations.
