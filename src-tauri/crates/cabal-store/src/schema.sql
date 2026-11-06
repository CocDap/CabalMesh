CREATE TABLE documents (
    name TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL
);
CREATE TABLE legacy_imports (
    name TEXT PRIMARY KEY,
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE intents (
    id TEXT PRIMARY KEY,
    network TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    action TEXT NOT NULL,
    asset TEXT NOT NULL,
    amount_raw TEXT NOT NULL,
    decimals INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);
CREATE INDEX intents_by_network_status ON intents(network,status,created_ms DESC);
CREATE TABLE networks (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL
);
CREATE TABLE assets (
    network TEXT NOT NULL,
    contract_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decimals INTEGER NOT NULL,
    PRIMARY KEY(network,contract_id),
    FOREIGN KEY(network) REFERENCES networks(id)
);
CREATE TABLE account_snapshots (
    account_id TEXT NOT NULL,
    network TEXT NOT NULL,
    native_wallet_raw TEXT,
    native_available_raw TEXT,
    wrapper_contract TEXT,
    updated_ms INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY(account_id,network),
    FOREIGN KEY(network) REFERENCES networks(id)
);
CREATE TABLE account_tokens (
    account_id TEXT NOT NULL,
    network TEXT NOT NULL,
    contract_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    decimals INTEGER NOT NULL,
    wallet_raw TEXT NOT NULL,
    in_intents_raw TEXT NOT NULL,
    PRIMARY KEY(account_id,network,contract_id),
    FOREIGN KEY(account_id,network) REFERENCES account_snapshots(account_id,network) ON DELETE CASCADE,
    FOREIGN KEY(network,contract_id) REFERENCES assets(network,contract_id)
);
