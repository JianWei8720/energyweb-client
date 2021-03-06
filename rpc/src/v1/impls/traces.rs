// Copyright 2015-2017 Parity Technologies (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! Traces api implementation.

use std::sync::Arc;

use ethcore::client::{MiningBlockChainClient, CallAnalytics, TransactionId, TraceId};
use ethcore::transaction::SignedTransaction;
use rlp::UntrustedRlp;

use jsonrpc_core::Result;
use jsonrpc_macros::Trailing;
use v1::Metadata;
use v1::traits::Traces;
use v1::helpers::{errors, fake_sign};
use v1::types::{TraceFilter, LocalizedTrace, BlockNumber, Index, CallRequest, Bytes, TraceResults, TraceOptions, H256};

fn to_call_analytics(flags: TraceOptions) -> CallAnalytics {
	CallAnalytics {
		transaction_tracing: flags.contains(&("trace".to_owned())),
		vm_tracing: flags.contains(&("vmTrace".to_owned())),
		state_diffing: flags.contains(&("stateDiff".to_owned())),
	}
}

/// Traces api implementation.
pub struct TracesClient<C> {
	client: Arc<C>,
}

impl<C> TracesClient<C> {
	/// Creates new Traces client.
	pub fn new(client: &Arc<C>) -> Self {
		TracesClient {
			client: client.clone(),
		}
	}
}

impl<C> Traces for TracesClient<C> where C: MiningBlockChainClient + 'static {
	type Metadata = Metadata;

	fn filter(&self, filter: TraceFilter) -> Result<Option<Vec<LocalizedTrace>>> {
		Ok(self.client.filter_traces(filter.into())
			.map(|traces| traces.into_iter().map(LocalizedTrace::from).collect()))
	}

	fn block_traces(&self, block_number: BlockNumber) -> Result<Option<Vec<LocalizedTrace>>> {
		Ok(self.client.block_traces(block_number.into())
			.map(|traces| traces.into_iter().map(LocalizedTrace::from).collect()))
	}

	fn transaction_traces(&self, transaction_hash: H256) -> Result<Option<Vec<LocalizedTrace>>> {
		Ok(self.client.transaction_traces(TransactionId::Hash(transaction_hash.into()))
			.map(|traces| traces.into_iter().map(LocalizedTrace::from).collect()))
	}

	fn trace(&self, transaction_hash: H256, address: Vec<Index>) -> Result<Option<LocalizedTrace>> {
		let id = TraceId {
			transaction: TransactionId::Hash(transaction_hash.into()),
			address: address.into_iter().map(|i| i.value()).collect()
		};

		Ok(self.client.trace(id)
			.map(LocalizedTrace::from))
	}

	fn call(&self, meta: Self::Metadata, request: CallRequest, flags: TraceOptions, block: Trailing<BlockNumber>) -> Result<TraceResults> {
		let block = block.unwrap_or_default();

		let request = CallRequest::into(request);
		let signed = fake_sign::sign_call(request, meta.is_dapp())?;

		self.client.call(&signed, to_call_analytics(flags), block.into())
			.map(TraceResults::from)
			.map_err(errors::call)
	}

	fn call_many(&self, meta: Self::Metadata, requests: Vec<(CallRequest, TraceOptions)>, block: Trailing<BlockNumber>) -> Result<Vec<TraceResults>> {
		let block = block.unwrap_or_default();

		let requests = requests.into_iter()
			.map(|(request, flags)| {
				let request = CallRequest::into(request);
				let signed = fake_sign::sign_call(request, meta.is_dapp())?;
				Ok((signed, to_call_analytics(flags)))
			})
			.collect::<Result<Vec<_>>>()?;

		self.client.call_many(&requests, block.into())
			.map(|results| results.into_iter().map(TraceResults::from).collect())
			.map_err(errors::call)
	}

	fn raw_transaction(&self, raw_transaction: Bytes, flags: TraceOptions, block: Trailing<BlockNumber>) -> Result<TraceResults> {
		let block = block.unwrap_or_default();

		let tx = UntrustedRlp::new(&raw_transaction.into_vec()).as_val().map_err(|e| errors::invalid_params("Transaction is not valid RLP", e))?;
		let signed = SignedTransaction::new(tx).map_err(errors::transaction)?;

		self.client.call(&signed, to_call_analytics(flags), block.into())
			.map(TraceResults::from)
			.map_err(errors::call)
	}

	fn replay_transaction(&self, transaction_hash: H256, flags: TraceOptions) -> Result<TraceResults> {
		self.client.replay(TransactionId::Hash(transaction_hash.into()), to_call_analytics(flags))
			.map(TraceResults::from)
			.map_err(errors::call)
	}
}
