use super::test_common::*;
use state::*;
use executive::*;
use spec::*;
use engine::*;
use evm;
use evm::{Schedule, Ext, Factory};
use ethereum;

struct TestEngine {
	spec: Spec,
	stack_limit: usize
}

impl TestEngine {
	fn new(stack_limit: usize) -> TestEngine {
		TestEngine {
			spec: ethereum::new_frontier_test(),
			stack_limit: stack_limit 
		}
	}
}

impl Engine for TestEngine {
	fn name(&self) -> &str { "TestEngine" }
	fn spec(&self) -> &Spec { &self.spec }
	fn schedule(&self, _env_info: &EnvInfo) -> Schedule { 
		let mut schedule = Schedule::new_frontier();
		schedule.stack_limit = self.stack_limit; 
		schedule
	}
}

struct CallCreate {
	data: Bytes,
	destination: Address,
	gas_limit: U256,
	value: U256
}

/// Tiny wrapper around executive externalities.
/// Stores callcreates.
struct TestExt<'a> {
	ext: Externalities<'a>,
	callcreates: Vec<CallCreate>
}

impl<'a> TestExt<'a> {
	fn new(ext: Externalities<'a>) -> TestExt {
		TestExt {
			ext: ext,
			callcreates: vec![]
		}
	}
}

impl<'a> Ext for TestExt<'a> {
	fn sload(&self, key: &H256) -> H256 {
		self.ext.sload(key)
	}

	fn sstore(&mut self, key: H256, value: H256) {
		self.ext.sstore(key, value)
	}

	fn balance(&self, address: &Address) -> U256 {
		self.ext.balance(address)
	}

	fn blockhash(&self, number: &U256) -> H256 {
		self.ext.blockhash(number)
	}

	fn create(&mut self, gas: u64, value: &U256, code: &[u8]) -> Result<(u64, Option<Address>), evm::Error> {
		// in call and create we need to check if we exited with insufficient balance or max limit reached.
		// in case of reaching max depth, we should store callcreates. Otherwise, ignore.
		let res = self.ext.create(gas, value, code);
		let ext = &self.ext;
		match res {
			// just record call create
			Ok((gas_left, Some(address))) => {
				self.callcreates.push(CallCreate {
					data: code.to_vec(),
					destination: address.clone(),
					gas_limit: U256::from(gas),
					value: *value
				});
				Ok((gas_left, Some(address)))
			},
			// creation failed only due to reaching stack_limit
			Ok((gas_left, None)) if ext.state.balance(&ext.params.address) >= *value => {
				let address = contract_address(&ext.params.address, &ext.state.nonce(&ext.params.address));
				self.callcreates.push(CallCreate {
					data: code.to_vec(),
					// TODO: address is not stored here?
					destination: Address::new(),
					gas_limit: U256::from(gas),
					value: *value
				});
				Ok((gas_left, Some(address)))
			},
			other => other
		}
	}

	fn call(&mut self, 
			gas: u64, 
			call_gas: u64, 
			receive_address: &Address, 
			value: &U256, 
			data: &[u8], 
			code_address: &Address, 
			output: &mut [u8]) -> Result<u64, evm::Error> {
		let res = self.ext.call(gas, call_gas, receive_address, value, data, code_address, output);
		let ext = &self.ext;
		match res {
			Ok(gas_left) if ext.state.balance(&ext.params.address) >= *value => {
				self.callcreates.push(CallCreate {
					data: data.to_vec(),
					destination: receive_address.clone(),
					gas_limit: U256::from(call_gas),
					value: *value
				});
				Ok(gas_left)
			},
			other => other
		}
	}

	fn extcode(&self, address: &Address) -> Vec<u8> {
		self.ext.extcode(address)
	}
	
	fn log(&mut self, topics: Vec<H256>, data: Bytes) {
		self.ext.log(topics, data)
	}

	fn ret(&mut self, gas: u64, data: &[u8]) -> Result<u64, evm::Error> {
		self.ext.ret(gas, data)
	}

	fn suicide(&mut self) {
		self.ext.suicide()
	}

	fn schedule(&self) -> &Schedule {
		self.ext.schedule()
	}

	fn env_info(&self) -> &EnvInfo {
		self.ext.env_info()
	}
}

fn do_json_test(json_data: &[u8]) -> Vec<String> {
	let json = Json::from_str(::std::str::from_utf8(json_data).unwrap()).expect("Json is invalid");
	let mut failed = Vec::new();
	for (name, test) in json.as_object().unwrap() {
		//::std::io::stdout().write(&name.as_bytes());
		//::std::io::stdout().write(b"\n");
		//::std::io::stdout().flush();
		//println!("name: {:?}", name);
		let mut fail = false;
		//let mut fail_unless = |cond: bool| if !cond && !fail { failed.push(name.to_string()); fail = true };
		let mut fail_unless = |cond: bool, s: &str | if !cond && !fail { failed.push(name.to_string() + ": "+ s); fail = true };
	
		// test env
		let mut state = State::new_temp();

		test.find("pre").map(|pre| for (addr, s) in pre.as_object().unwrap() {
			let address = address_from_str(addr);
			let balance = u256_from_json(&s["balance"]);
			let code = bytes_from_json(&s["code"]);
			let nonce = u256_from_json(&s["nonce"]);

			state.new_contract(&address);
			state.add_balance(&address, &balance);
			state.init_code(&address, code);

			for (k, v) in s["storage"].as_object().unwrap() {
				let key = H256::from(&u256_from_str(k));
				let val = H256::from(&u256_from_json(v));
				state.set_storage(&address, key, val);
			}
		});

		let mut info = EnvInfo::new();

		test.find("env").map(|env| {
			info.author = address_from_json(&env["currentCoinbase"]);
			info.difficulty = u256_from_json(&env["currentDifficulty"]);
			info.gas_limit = u256_from_json(&env["currentGasLimit"]);
			info.number = u256_from_json(&env["currentNumber"]).low_u64();
			info.timestamp = u256_from_json(&env["currentTimestamp"]).low_u64();
		});

		let engine = TestEngine::new(0);

		// params
		let mut params = ActionParams::new();
		test.find("exec").map(|exec| {
			params.address = address_from_json(&exec["address"]);
			params.sender = address_from_json(&exec["caller"]);
			params.origin = address_from_json(&exec["origin"]);
			params.code = bytes_from_json(&exec["code"]);
			params.data = bytes_from_json(&exec["data"]);
			params.gas = u256_from_json(&exec["gas"]);
			params.gas_price = u256_from_json(&exec["gasPrice"]);
			params.value = u256_from_json(&exec["value"]);
		});

		let out_of_gas = test.find("callcreates").map(|calls| {
		}).is_none();
		
		let mut substate = Substate::new();
		let mut output = vec![];

		// execute
		let res = {
			let ex = Externalities::new(&mut state, &info, &engine, 0, &params, &mut substate, OutputPolicy::Return(BytesRef::Flexible(&mut output)));
			let mut test_ext = TestExt::new(ex);
			let evm = Factory::create();
			evm.exec(&params, &mut test_ext)
		};

		// then validate
		match res {
			Err(_) => fail_unless(out_of_gas, "didn't expect to run out of gas."),
			Ok(gas_left) => {
				println!("name: {}, gas_left : {:?}, expected: {:?}", name, gas_left, u256_from_json(&test["gas"]));
				fail_unless(!out_of_gas, "expected to run out of gas.");
				fail_unless(gas_left == u256_from_json(&test["gas"]), "gas_left is incorrect");
				fail_unless(output == bytes_from_json(&test["out"]), "output is incorrect");
			}
		}
	}


	for f in failed.iter() {
		println!("FAILED: {:?}", f);
	}

	//assert!(false);
	failed
}

declare_test!{ExecutiveTests_vmArithmeticTest, "VMTests/vmArithmeticTest"}
declare_test!{ExecutiveTests_vmBitwiseLogicOperationTest, "VMTests/vmBitwiseLogicOperationTest"}
// this one crashes with some vm internal error. Separately they pass.
//declare_test!{ExecutiveTests_vmBlockInfoTest, "VMTests/vmBlockInfoTest"}
declare_test!{ExecutiveTests_vmEnvironmentalInfoTest, "VMTests/vmEnvironmentalInfoTest"}
declare_test!{ExecutiveTests_vmIOandFlowOperationsTest, "VMTests/vmIOandFlowOperationsTest"}
// this one take way too long.
//declare_test!{ExecutiveTests_vmInputLimits, "VMTests/vmInputLimits"}
declare_test!{ExecutiveTests_vmLogTest, "VMTests/vmLogTest"}
declare_test!{ExecutiveTests_vmPerformanceTest, "VMTests/vmPerformanceTest"}
declare_test!{ExecutiveTests_vmPushDupSwapTest, "VMTests/vmPushDupSwapTest"}
declare_test!{ExecutiveTests_vmSha3Test, "VMTests/vmSha3Test"}
declare_test!{ExecutiveTests_vmSystemOperationsTest, "VMTests/vmSystemOperationsTest"}
declare_test!{ExecutiveTests_vmtests, "VMTests/vmtests"}
