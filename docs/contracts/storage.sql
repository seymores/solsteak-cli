-- Schema 1. Application executes this inside an IMMEDIATE migration transaction.
-- Canonical u64 text is validated at the Rust boundary; numeric reward columns
-- additionally reject noncanonical or out-of-range text at the SQL boundary.
CREATE TABLE schema_history(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
CREATE TABLE networks(
  genesis_hash TEXT PRIMARY KEY,
  cluster TEXT NOT NULL CHECK(cluster='mainnet'),
  last_epoch TEXT NOT NULL,
  observed_at INTEGER
);
CREATE TABLE snapshots(
  id INTEGER PRIMARY KEY,
  network TEXT NOT NULL REFERENCES networks(genesis_hash),
  address TEXT NOT NULL,
  selection TEXT NOT NULL CHECK(selection IN ('authority','direct')),
  complete INTEGER NOT NULL CHECK(complete IN (0,1)),
  report_json TEXT NOT NULL CHECK(json_valid(report_json)),
  ordering_json TEXT NOT NULL CHECK(json_valid(ordering_json))
);
CREATE INDEX snapshots_address ON snapshots(network,address,id);
CREATE TABLE address_heads(
  network TEXT NOT NULL REFERENCES networks(genesis_hash),
  address TEXT NOT NULL,
  latest_attempt INTEGER NOT NULL REFERENCES snapshots(id),
  latest_complete INTEGER REFERENCES snapshots(id),
  PRIMARY KEY(network,address)
);
CREATE TABLE account_observations(
  snapshot INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
  address TEXT NOT NULL,
  account_json TEXT NOT NULL CHECK(json_valid(account_json)),
  source_json TEXT NOT NULL CHECK(json_valid(source_json)),
  PRIMARY KEY(snapshot,address)
);
CREATE TABLE members(
  snapshot INTEGER NOT NULL,
  address TEXT NOT NULL,
  relationship TEXT CHECK(relationship IN ('staker','withdrawer','both')),
  PRIMARY KEY(snapshot,address),
  FOREIGN KEY(snapshot,address) REFERENCES account_observations(snapshot,address) ON DELETE CASCADE
);
CREATE TABLE validators(
  network TEXT NOT NULL REFERENCES networks(genesis_hash),
  vote_address TEXT NOT NULL,
  validator_json TEXT NOT NULL CHECK(json_valid(validator_json)),
  source_json TEXT NOT NULL CHECK(json_valid(source_json)),
  PRIMARY KEY(network,vote_address)
);
CREATE TABLE rewards(
  network TEXT NOT NULL REFERENCES networks(genesis_hash),
  address TEXT NOT NULL,
  epoch TEXT NOT NULL CHECK(length(epoch) BETWEEN 1 AND 20 AND epoch NOT GLOB '*[^0-9]*' AND (epoch='0' OR substr(epoch,1,1)!='0') AND (length(epoch)<20 OR epoch<='18446744073709551615')),
  amount TEXT NOT NULL CHECK(length(amount) BETWEEN 1 AND 20 AND amount NOT GLOB '*[^0-9]*' AND (amount='0' OR substr(amount,1,1)!='0') AND (length(amount)<20 OR amount<='18446744073709551615')),
  post_balance TEXT NOT NULL CHECK(length(post_balance) BETWEEN 1 AND 20 AND post_balance NOT GLOB '*[^0-9]*' AND (post_balance='0' OR substr(post_balance,1,1)!='0') AND (length(post_balance)<20 OR post_balance<='18446744073709551615')),
  effective_slot TEXT NOT NULL CHECK(length(effective_slot) BETWEEN 1 AND 20 AND effective_slot NOT GLOB '*[^0-9]*' AND (effective_slot='0' OR substr(effective_slot,1,1)!='0') AND (length(effective_slot)<20 OR effective_slot<='18446744073709551615')),
  commission INTEGER CHECK(commission BETWEEN 0 AND 100),
  record_json TEXT NOT NULL CHECK(json_valid(record_json)),
  source_json TEXT NOT NULL CHECK(json_valid(source_json)),
  PRIMARY KEY(network,address,epoch)
);
CREATE TABLE reward_coverage(
  network TEXT NOT NULL REFERENCES networks(genesis_hash),
  address TEXT NOT NULL,
  epoch TEXT NOT NULL,
  observed_at INTEGER,
  state TEXT NOT NULL CHECK(state IN ('recorded','no_data','failed','not_queried')),
  error_code TEXT,
  retry_eligible INTEGER NOT NULL CHECK(retry_eligible IN (0,1)),
  PRIMARY KEY(network,address,epoch)
);
