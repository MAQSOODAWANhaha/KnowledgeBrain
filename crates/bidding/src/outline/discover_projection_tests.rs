#[test]
fn compact_blank_ranges_preserve_canonical_cells_and_request_dictionary() {
    let cells = (0..7).flat_map(|row| (0..11).map(move |column|
        json!({"row":row,"column":column,"row_span":1,"col_span":1,
            "text":if row == 0 {format!("header {column}")} else {String::new()},
            "header_role":if row == 0 {"column_header"} else {"none"}}))).collect::<Vec<_>>();
    let input = input(vec![source("t",0,"","one")],vec![json!({"source_unit_revision_id":"t",
        "form_definition_revision_id":"table","definition":{"row_count":7,"column_count":11,"cells":cells}})]);
    let before = input_digest(&input).unwrap();
    let mut work = DiscoverWork::plan(&input,32768);
    let pack = work.claim(1).remove(0);
    let canonical = work.canonical_session(&pack.id).unwrap();
    assert_eq!(canonical["pack"]["atoms"][0]["carrier"]["cells"].as_array().unwrap().len(),77);
    let wire = work.session(&input,&pack.id).unwrap();
    let atom = &wire["pack"]["atoms"][0];
    assert_eq!(atom["cells"].as_array().unwrap().len(),11);
    assert!(atom["carrier"].get("cells").is_none());
    let ranges = atom["table_structure"]["blank_source_ranges"].as_object().unwrap();
    assert_eq!(ranges.len(),1);
    let mut restored = BTreeSet::new();
    for range in ranges.values() {
        assert_eq!(range["row_count"],6);
        for row in range["start_row"].as_u64().unwrap()..range["end_row_exclusive"].as_u64().unwrap() {
            for col in range["start_column"].as_u64().unwrap()..range["end_column_exclusive"].as_u64().unwrap() {
                assert!(restored.insert((row,col)));
            }
        }
    }
    let original = canonical["pack"]["atoms"][0]["carrier"]["cells"].as_array().unwrap().iter()
        .filter(|c| c["start_byte"] == c["end_byte"])
        .map(|c|(c["anchor_row"].as_u64().unwrap(),c["anchor_column"].as_u64().unwrap())).collect::<BTreeSet<_>>();
    assert_eq!(restored,original);
    let mut request = json!({"messages":[{"role":"system","content":"test"},
        {"role":"user","content":json!({"reading_packs":[wire.clone(),wire.clone()]}).to_string()},
        {"role":"tool","content":json!({"session":wire,"unmapped_forms":[{"form_id":"table","title":"",
            "header":(0..11).map(|n|format!("header {n}")).collect::<Vec<_>>() }],
            "excerpts":[{"locator":{"table_id":"table","anchor_row":0,"anchor_column":0,"row_span":1,"column_span":1}}],
            "review":[{"structural_receipt_id":"receipt","table_id":"table","row_count":7,"column_count":11,
                "cell":{"anchor_row":0,"anchor_column":0,"row_span":1,"column_span":1,"header_role":"column_header","start_byte":0,"end_byte":8}}]}).to_string()}]});
    compact_request(&mut request).unwrap();
    let once = request.clone();
    compact_request(&mut request).unwrap();
    assert_eq!(once,request);
    let brief: Value = serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(brief["table_structures"].as_array().unwrap().len(),1);
    assert_eq!(brief["table_structures"][0]["anchors"].as_object().unwrap().len(),11);
    let serialized = request.to_string();
    assert_eq!(serialized.matches("header 0").count(),1);
    assert_eq!(serialized.matches("row_span").count(),11);
    assert_eq!(serialized.matches("end_row_exclusive").count(),1);
    assert_eq!(input_digest(&input).unwrap(),before);
    assert_eq!(work.canonical_session(&pack.id).unwrap(),canonical);
}

#[test]
fn compact_projection_preserves_whitespace_merges_headers_and_partial_rows() {
    let values = ["H", "", " ", "0", "false", "☐", "签字：", "=A1", ""];
    let mut cells = values.iter().enumerate().map(|(n,text)|json!({"row":n / 3,"column":n % 3,
        "row_span":1,"col_span":1,"text":text,"header_role":"none"})).collect::<Vec<_>>();
    cells[8]["row_span"] = json!(2);
    let input = input(vec![source("t",0,"","one")],vec![json!({"source_unit_revision_id":"t",
        "form_definition_revision_id":"table","definition":{"row_count":4,"column_count":3,"cells":cells}})]);
    let packs = plan_packs(&input,32768).unwrap();
    let wire = materialize_pack(&input,&packs[0]).unwrap();
    assert_eq!(wire["atoms"][0]["cells"].as_array().unwrap().len(),9);
    assert!(wire["atoms"][0].get("blank_range_refs").is_none());
    assert_eq!(wire["atoms"][0]["table_structure"]["anchors"]["0,2"]["header_fragments"]["0,1"]," ");
    assert_eq!(wire["atoms"][0]["cells"][3]["anchor_row"],1);
    assert_eq!(wire["atoms"][0]["cells"][3]["text"],"0");
}

#[test]
fn partial_row_ranges_do_not_shift_columns_or_compress_controls() {
    let cells = vec![json!({"row":0,"column":0,"text":"H"}),
        json!({"row":1,"column":0,"text":""}),json!({"row":1,"column":1,"text":"0"}),
        json!({"row":1,"column":2,"text":""}),json!({"row":2,"column":0,"text":"false"}),
        json!({"row":2,"column":1,"text":"","checkbox":false}),json!({"row":2,"column":2,"text":" "})];
    let input = input(vec![source("t",0,"","one")],vec![json!({"source_unit_revision_id":"t",
        "form_definition_revision_id":"table","definition":{"row_count":3,"column_count":3,"cells":cells}})]);
    let packs = plan_packs(&input,32768).unwrap();
    let wire = materialize_pack(&input,&packs[0]).unwrap();
    let atom = &wire["atoms"][0];
    assert_eq!(atom["table_structure"]["blank_source_ranges"].as_object().unwrap().len(),2);
    assert_eq!(atom["cells"][1]["anchor_row"],1);
    assert_eq!(atom["cells"][1]["anchor_column"],1);
    assert_eq!(atom["cells"][1]["text"],"0");
    assert_eq!(atom["cells"].as_array().unwrap().len(),5);
    assert_eq!(atom["table_structure"]["anchors"]["2,1"]["checkbox"],false);
}

#[test]
#[ignore = "requires private frozen input and explicit private output directory"]
fn private_actual_projection_roundtrip() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let read = |path: &str| -> Value { serde_json::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap() };
    let input: FrozenInput = serde_json::from_value(read("actual-frozen/frozen-input.json")).unwrap();
    let checkpoint = read("live-run/checkpoint.json");
    let work: DiscoverWork = serde_json::from_value(checkpoint["outline_run"]["reading_packs"].clone()).unwrap();
    let mut blank_count = 0;
    let mut all_sessions = Vec::new();
    for id in work.pack_ids() {
        let wire = work.session(&input,&id).unwrap();
        let canonical = work.canonical_session(&id).unwrap();
        for (atom,original) in wire["pack"]["atoms"].as_array().unwrap().iter()
            .zip(canonical["pack"]["atoms"].as_array().unwrap()) {
            if original["carrier"]["kind"] != "grid" { continue; }
            let mut coordinates = BTreeSet::new();
            for cell in atom["cells"].as_array().unwrap() {
                coordinates.insert((cell["anchor_row"].as_u64().unwrap(),cell["anchor_column"].as_u64().unwrap()));
                let table_id = original["carrier"]["table_id"].as_str().unwrap();
                let def = super::super::evidence::table_definition(&input,table_id).unwrap();
                let row = cell["anchor_row"].as_u64().unwrap() as usize;
                let col = cell["anchor_column"].as_u64().unwrap() as usize;
                let source = super::super::evidence::grid_cell(def,row,col).unwrap();
                let start = cell["start_byte"].as_u64().unwrap() as usize;
                let end = cell["end_byte"].as_u64().unwrap() as usize;
                let coordinate = format!("{row},{col}");
                let interval = format!("{start},{end}");
                let text = cell.get("text").unwrap_or(&atom["table_structure"]["anchors"][coordinate]["header_fragments"][interval]).as_str().unwrap();
                assert_eq!(text, &source["text"].as_str().unwrap()[start..end]);
            }
            for range in atom["table_structure"]["blank_source_ranges"].as_object().into_iter().flat_map(|m|m.values()) {
                for row in range["start_row"].as_u64().unwrap()..range["end_row_exclusive"].as_u64().unwrap() {
                    for col in range["start_column"].as_u64().unwrap()..range["end_column_exclusive"].as_u64().unwrap() {
                        assert!(coordinates.insert((row,col)));
                        let def = super::super::evidence::table_definition(&input,original["carrier"]["table_id"].as_str().unwrap()).unwrap();
                        let source = super::super::evidence::grid_cell(def,row as usize,col as usize).unwrap();
                        assert_eq!(source["text"],"");
                        assert_eq!(source["row_span"],1);
                        assert_eq!(source["col_span"],1);
                        blank_count += 1;
                    }
                }
            }
            let expected = original["carrier"]["cells"].as_array().unwrap().iter()
                .map(|c|(c["anchor_row"].as_u64().unwrap(),c["anchor_column"].as_u64().unwrap())).collect::<BTreeSet<_>>();
            assert_eq!(coordinates,expected);
        }
        all_sessions.push(wire);
    }
    let mut request = read("live-run/prepared-request.json");
    let mut brief: Value = serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    let ids = brief["reading_packs"].as_array().unwrap().iter().map(|s|s["pack"]["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    brief["reading_packs"] = json!(ids.iter().map(|id|work.session(&input,id).unwrap()).collect::<Vec<_>>());
    request["messages"][1]["content"] = json!(brief.to_string());
    compact_request(&mut request).unwrap();
    std::fs::write(root.join("fixed-scope-compact-request.json"),serde_json::to_vec(&request).unwrap()).unwrap();
    std::fs::write(root.join("compact-all-original-sessions.json"),serde_json::to_vec(&all_sessions).unwrap()).unwrap();
    std::fs::write(root.join("compact-coordinate-audit.json"),json!({"pass":true,"original_pack_count":all_sessions.len(),
        "compressed_empty_anchors":blank_count,"canonical_input_digest":input_digest(&input).unwrap(),"canonical_cells_unchanged":true}).to_string()).unwrap();
}

#[test]
fn host_evidence_keys_resolve_exactly_and_reject_blank_or_foreign_sources() {
    let input = input(vec![source("t",0,"","one")],vec![json!({"source_unit_revision_id":"t",
        "form_definition_revision_id":"table","definition":{"row_count":2,"column_count":1,
        "cells":[{"row":0,"column":0,"text":"甲😀乙","header_role":"column_header"},{"row":1,"column":0,"text":"","header_role":"none"}]}})]);
    let mut work = DiscoverWork::plan(&input,32768);
    let pack = work.claim(1).remove(0);
    let reference=atom_scope(&pack.atoms[0],&pack.input_digest,true).remove(0);
    let valid=evidence_key(&pack,&pack.atoms[0],&reference).unwrap();
    let atom=atom_key(&pack,&pack.atoms[0]).unwrap();
    let make = |key:&str|json!({"pack_id":pack.id,"call_id":"handles","claim_token":pack.claim_token,"pack_revision":pack.pack_revision,
        "requirements":[{"description":"Requirement","kind":"format","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom},"evidence":[{"evidence_key":key}]}],"no_requirement_reason":null,"inspected_atom_ids":[{"atom_key":atom}]});
    assert_eq!(atom_scope(&pack.atoms[0],&pack.input_digest,true).len(),1,"blank cell has no selectable key");
    let mut bad=make("ev_invalid");let error=resolve_submission_handles(&pack,&mut bad).unwrap_err();assert!(error.contains("requirements[0].evidence[0]"));assert!(error.contains("invalid or stale"));
    let mut foreign=pack.clone();foreign.id.push('x');assert!(resolve_submission_handles(&foreign,&mut make(&valid)).is_err());
    let mut duplicate=make(&valid);duplicate["inspected_atom_ids"]=json!([{"atom_key":atom},{"atom_key":atom}]);assert!(resolve_submission_handles(&pack,&mut duplicate).is_err());
    let mut body=make(&valid);resolve_submission_handles(&pack,&mut body).unwrap();assert_eq!(body["requirements"][0]["source_section_id"],"one");assert_eq!(body["requirements"][0]["evidence"][0],json!(reference));assert_eq!(body["requirements"][0]["evidence"][0]["end_byte"],10);
    let mut slot = Some(work);
    let mut keyed = make(&valid);
    let reference = atom_scope(&pack.atoms[0],&pack.input_digest,true).remove(0);
    keyed["requirements"][0]["evidence"] = json!([{ "evidence_key":evidence_key(&pack,&pack.atoms[0],&reference).unwrap() }]);
    let (claim,operation)=slot.as_ref().unwrap().model_handles(&pack.id).unwrap();
    keyed["claim_token"]=json!(claim);keyed["call_id"]=json!(operation);
    let result = apply_pack_tool(&mut slot,&input,"submit_pack_scan",&keyed).unwrap();
    assert_eq!(result["status"],"committed");
    assert_eq!(slot.unwrap().requirement_records().len(),1);
}

#[tokio::test]
#[ignore = "explicitly authorized private real-PDF two-call regression only"]
async fn private_real_pdf_handle_regression() {
    assert_eq!(std::env::var("KB_ALLOW_PRIVATE_LIVE").as_deref(),Ok("1"));
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let out = root.join("targeted-handles-v1");
    std::fs::create_dir(&out).expect("new diagnostic run required; do not repeat billed calls");
    let input: FrozenInput = serde_json::from_slice(&std::fs::read(root.join("actual-frozen/frozen-input.json")).unwrap()).unwrap();
    crate::analysis::tools::validate_input(&input).unwrap();
    let limits: Value = serde_json::from_slice(&std::fs::read(root.join("compact-live-limits.json")).unwrap()).unwrap();
    let provider = crate::authoring_runtime::AuthoringRuntimeContractV1::resolve_tools_from_environment().unwrap();
    let config = crate::analysis::agent::Config::with_provider(provider,serde_json::from_value(limits["extraction"].clone()).unwrap()).unwrap();
    let mut work = DiscoverWork::plan_with_budget(&input,&|sessions| Ok(sessions.iter().all(|s|s["pack"]["atoms"].as_array().unwrap().len()<=2)));
    assert!(work.planning_error().is_none(),"{:?}",work.planning_error());
    let packs = work.claim(1000);
    let selection: Value = serde_json::from_slice(&std::fs::read(root.join("targeted-selection.json")).unwrap()).unwrap();
    let clarification = input.source_units.iter().find(|s|Some(s.source_unit_revision_id.as_str()) == selection["text_source_id"].as_str()).unwrap();
    let text_pack = packs.iter().find(|p|p.atoms.iter().any(|a|matches!(&a.carrier,PackCarrier::Text {evidence:EvidenceRef::Text {unit_id,..}} if unit_id == &clarification.source_unit_revision_id))).unwrap();
    let grid_pack = packs.iter().find(|p|p.atoms.iter().any(|a|matches!(&a.carrier,PackCarrier::Grid {table_id,..} if Some(table_id.as_str()) == selection["grid_table_id"].as_str()))).unwrap();
    let selected = [text_pack.id.clone(),grid_pack.id.clone()];
    let save = |name:&str,value:&Value| std::fs::write(out.join(name),serde_json::to_vec_pretty(value).unwrap()).unwrap();
    save("contract.json",&json!({"kind":"two-call real-PDF contract regression; not full-document acceptance","config":config,
        "input_digest":input_digest(&input).unwrap(),"selected_pack_ids":selected,"max_physical_calls":2,"full_input_unchanged":true}));
    let mut outcomes = Vec::new();
    for (index,id) in selected.iter().enumerate() {
        let session = work.session(&input,id).unwrap();
        let instruction = "本次只针对给出的真实阅读包验证契约。读完全部来源，逐项提取实际应答义务，然后调用 submit_pack。优先用 {pack_id:当前包ID,atom_ref:N,evidence_index:M} 选择主机标出的非空证据，不推算字节偏移，不拼接来源ID。source_section_id 和 inspected_atom_ids 可用 {pack_id:当前包ID,atom_ref:N}。空白格没有证据索引，不得作为引文；原文空白行数量不是必须提交条目数量。";
        let bytes = crate::agent_runtime::chat::prepare(&config.provider,vec![
            json!({"role":"system","content":crate::outline::agent::system_prompt(crate::outline::agent::Duty::Discover)}),
            json!({"role":"user","content":json!({"reading_packs":[session],"instruction":instruction}).to_string()})],crate::outline::agent::schemas_for(crate::outline::agent::Duty::Discover)).await.unwrap();
        let mut body:Value = serde_json::from_slice(&bytes).unwrap();
        compact_request(&mut body).unwrap();
        body["tool_choice"] = json!({"type":"function","function":{"name":"submit_pack"}});
        let accounting = crate::agent_runtime::chat::estimate_request_tokens(&body,&config.limits.tokenizer,config.limits.image_token_reserve,config.limits.token_safety_margin).unwrap();
        assert!(accounting.total_context_tokens <= config.limits.max_context_tokens);
        save(&format!("request-{}.json",index+1),&body);
        save(&format!("accounting-{}.json",index+1),&serde_json::to_value(accounting).unwrap());
        let start = std::time::Instant::now();
        let result = crate::analysis::agent::Model::turn(&crate::analysis::agent::ConfiguredModel,&config,&serde_json::to_vec(&body).unwrap()).await;
        let response = match result {
            Ok(response)=>response,
            Err(error)=>{save(&format!("response-{}.json",index+1),&json!({"error":error.to_string(),"seconds":start.elapsed().as_secs_f64()}));break;}
        };
        save(&format!("response-{}.json",index+1),&json!({"response":response,"seconds":start.elapsed().as_secs_f64()}));
        let mut receipts = Vec::new();
        for call in response.tool_calls {
            let mut args:Value = serde_json::from_str(&call.arguments).unwrap();
            let result = crate::outline::agent::validate_arguments(&call.name,&args).and_then(|_| {
                if call.name != "submit_pack" || args["pack_id"] != *id || args["repair"] != false {return Err("wrong regression tool, pack, or state".into());}
                args.as_object_mut().unwrap().remove("repair");
                let mut slot = Some(work.clone());
                let result=apply_pack_tool(&mut slot,&input,"submit_pack_scan",&args);
                if result.is_ok() {work=slot.unwrap();}
                result
            });
            receipts.push(json!({"result":result}));
        }
        outcomes.push(json!({"pack_id":id,"status":work.status(id),"receipts":receipts}));
        save("outcomes.json",&json!(outcomes));
        save("canonical-work.json",&serde_json::to_value(&work).unwrap());
    }
    save("summary.json",&json!({"outcomes":outcomes,"physical_call_cap":2,"full_document_acceptance":false}));
}

#[test]
fn handle_selection_keeps_original_section_and_source_validation() {
    let input = input(vec![source("unit-a",0,"甲","section-a"),source("unit-b",1,"乙","section-b")],vec![]);
    let mut work = DiscoverWork::plan(&input,32768);
    let pack = work.claim(1).remove(0);
    let mut body = json!({"pack_id":pack.id,"call_id":"cross-section","claim_token":pack.claim_token,"pack_revision":pack.pack_revision,
        "requirements":[{"description":"must respond","kind":"format","obligation_strength":"mandatory","extraction_quality":"explicit",
            "source_section_id":{"atom_key":atom_key(&pack,&pack.atoms[1]).unwrap()},"evidence":[{"evidence_key":evidence_key(&pack,&pack.atoms[0],&atom_scope(&pack.atoms[0],&pack.input_digest,true)[0]).unwrap()}]}],
        "no_requirement_reason":null,"inspected_atom_ids":[{"atom_key":atom_key(&pack,&pack.atoms[0]).unwrap()},{"atom_key":atom_key(&pack,&pack.atoms[1]).unwrap()}]});
    let mut resolved = body.clone();
    resolve_submission_handles(&pack,&mut resolved).unwrap();
    assert_eq!(resolved["requirements"][0]["evidence"][0]["unit_id"],"unit-a");
    assert_eq!(resolved["requirements"][0]["source_section_id"],"section-b");
    let mut slot = Some(work);
    let (claim,operation)=slot.as_ref().unwrap().model_handles(&pack.id).unwrap();
    body["claim_token"]=json!(claim);body["call_id"]=json!(operation);
    let result = apply_pack_tool(&mut slot,&input,"submit_pack_scan",&body).unwrap();
    assert_eq!(result["ok"],false);
    assert!(result["feedback"]["errors"].as_array().unwrap().iter().any(|e|e["code"]=="outside_section"));
    assert!(slot.as_ref().unwrap().requirement_records().is_empty());
    let mut guessed = body;
    guessed["call_id"] = json!("invented-source");
    guessed["requirements"][0]["source_section_id"] = json!("section-a");
    guessed["requirements"][0]["evidence"][0] = json!({"kind":"text","input_digest":pack.input_digest,"unit_id":"not-a-source","start_byte":0,"end_byte":1});
    let before=slot.clone();assert!(apply_pack_tool(&mut slot,&input,"repair_pack_scan",&guessed).is_err());assert_eq!(slot,before,"raw forged source cannot mutate a pack");
}

mod private_semantic_regression {
// Private bounded Check/reopen regression using an isolated checkpoint fixture.
use crate::{analysis::{FrozenInput,agent::{self,Model}},outline::{self,agent::{apply_for_duty,Duty}},authoring_runtime::AuthoringRuntimeContractV1};
use serde_json::{Value,json};
use std::{fs,path::PathBuf};
fn read<T:serde::de::DeserializeOwned>(p:PathBuf)->T {serde_json::from_slice(&fs::read(p).unwrap()).unwrap()}
fn save(p:PathBuf,v:&impl serde::Serialize){fs::write(p,serde_json::to_vec_pretty(v).unwrap()).unwrap();}
#[tokio::test]
#[ignore = "authorized two-call private semantic regression only"]
async fn private_real_semantic_check_repair(){
 assert_eq!(std::env::var("KB_ALLOW_PRIVATE_LIVE").as_deref(),Ok("1"));
 let root=PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
 let case=std::env::var("KB_SEMANTIC_CASE").unwrap_or_else(|_|"postal-v3".into());
 assert!(case.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'));
 let out=root.join(format!("targeted-semantic-{case}"));fs::create_dir(&out).expect("new directory required: never repeat billed calls");
 let input:FrozenInput=read(root.join("actual-frozen/frozen-input.json"));
 let mut state:agent::Checkpoint=read(root.join("semantic-complete-fixture-checkpoint.json"));
 let selection:Value=read(root.join(format!("semantic-selection-{case}.json")));let id=selection["requirement_id"].as_str().unwrap();let pack_id=selection["pack_id"].as_str().unwrap();
 let record=state.outline_run.reading_packs.as_ref().unwrap().requirement_records()[id].clone();
 let limits:Value=read(root.join("handles-full-live-limits.json"));
 let config=agent::Config::with_provider(AuthoringRuntimeContractV1::resolve_tools_from_environment().unwrap(),serde_json::from_value(limits["extraction"].clone()).unwrap()).unwrap();
 // The isolated fixture supplies an explicit visible response target. It does not claim full Organize acceptance.
 state.outline_run.tool_draft=outline::tools::Draft::default();
 state.outline_run.tool_draft.required_requirement_ids.insert(id.into());
 state.outline_run.tool_draft.required_pack_ids.insert(pack_id.into());
 state.outline_run.tool_draft.fulfillments.push(serde_json::from_value(json!({"requirement_id":id,"primary_response_chapter_id":"fixture","target_refs":[{"type":"manual_task","task_id":"fixture-response","description":record.description}]})).unwrap());
 let mut refs=record.evidence.clone();
 if let Some(extra)=selection.get("supplemental_refs") {refs.extend(serde_json::from_value::<Vec<outline::evidence::EvidenceRef>>(extra.clone()).unwrap());}
 let source=apply_for_duty(&input,&mut state,"read_evidence",&json!({"refs":refs,"max_bytes":16384}),Duty::Check).unwrap();
 let mut payload=json!({"scope":"isolated real-evidence Check handler regression, not full Organize acceptance","requirement_id":id,"requirement":record,"response_target":state.outline_run.tool_draft.fulfillments,"fresh_read_evidence_result":source.clone(),"instruction":"独立检查每项主张是否由原文支持，逐项核对否定、条件、例外、适用对象和响应充分性。调用submit_review报告有原文证据的问题，requirement_ids仅本条，pack_ids留空。不要根据已有描述倒推原文含义。"});
 for index in 1..=2 {
  let duty=if index==1{Duty::Check}else{Duty::Discover};let tool=if index==1{"submit_review"}else{"submit_pack"};
  payload["semantic_contract"]=json!(outline::agent::semantic_guidance(duty));
  let bytes=crate::agent_runtime::chat::prepare(&config.provider,vec![json!({"role":"system","content":outline::agent::system_prompt(duty)}),json!({"role":"user","content":payload.to_string()})],outline::agent::schemas_for(duty)).await.unwrap();
  let mut body:Value=serde_json::from_slice(&bytes).unwrap();crate::outline::discover::compact_request(&mut body).unwrap();body["tool_choice"]=json!({"type":"function","function":{"name":tool}});
  let accounting=crate::agent_runtime::chat::estimate_request_tokens(&body,&config.limits.tokenizer,config.limits.image_token_reserve,config.limits.token_safety_margin).unwrap();
  save(out.join(format!("accounting-{index}.json")),&accounting);assert!(accounting.total_context_tokens<=config.limits.max_context_tokens);
  save(out.join(format!("request-{index}.json")),&body);let start=std::time::Instant::now();
  let result=agent::ConfiguredModel.turn(&config,&serde_json::to_vec(&body).unwrap()).await;
  let response=match result{Ok(v)=>v,Err(e)=>{save(out.join(format!("response-{index}.json")),&json!({"error":e.to_string(),"seconds":start.elapsed().as_secs_f64()}));return}};
  save(out.join(format!("response-{index}.json")),&json!({"response":response,"seconds":start.elapsed().as_secs_f64()}));state.turn+=1;
  let mut accepted=false;
  for call in &response.tool_calls {let args:Value=serde_json::from_str(&call.arguments).unwrap();let result=apply_for_duty(&input,&mut state,&call.name,&args,duty);accepted|=result.as_ref().is_ok_and(|value|value["ok"]!=false);save(out.join(format!("host-result-{index}.json")),&json!({"tool":call.name,"result":result}));}
  save(out.join(format!("checkpoint-{index}.json")),&state);
  if index==1 {
   let issues=state.outline_run.tool_draft.review_issues.clone();save(out.join("check-issues.json"),&issues);
   if !accepted||issues.is_empty(){save(out.join("summary.json"),&json!({"check_detected_issue":false,"repair_sent":false}));return}
   let revision=state.outline_run.reading_packs.as_ref().unwrap().session(&input,pack_id).unwrap()["pack"]["pack_revision"].as_u64().unwrap();
   outline::agent::reopen_discovery_pack(&input,&mut state,pack_id,revision,"private-semantic-reopen-1").unwrap();
   let session=state.outline_run.reading_packs.as_ref().unwrap().session(&input,pack_id).unwrap();
   payload=json!({"reading_packs":[session],"review_issues":issues,"supplemental_original_evidence":source,"instruction":"根据复核问题重新阅读全部包，修正受影响的义务并保留其他真实要求。逐条结合否定、条件、例外与适用对象判定是否应答；项目明确例外优先于通用模板，未确定适用时标为待复核，不自行添加义务。通过submit_pack提交完整修复，遵守当前session状态。"});
  }else{save(out.join("summary.json"),&json!({"check_detected_issue":true,"repair_host_accepted":accepted,"semantic_repair_requires_independent_audit":true,"full_workflow_acceptance":false}));}
 }
}

}

#[test]
fn host_submission_identity_is_stable_for_retries_and_new_after_reopen() {
    let input=input(vec![source("a",0,"required evidence","section")],vec![]);
    let mut work=DiscoverWork::plan(&input,8192);let pack=work.claim(1).remove(0);
    let operation=work.session(&input,&pack.id).unwrap()["submission_operation_id"].as_str().unwrap().to_string();
    assert_eq!(work.session(&input,&pack.id).unwrap()["submission_operation_id"],operation);
    let submit=submission(&pack,&operation,vec![req(&input,"a",0,17,"section")]);
    work.submit(&input,&pack.id,submit.clone()).unwrap();let committed=work.clone();
    work.submit(&input,&pack.id,submit).unwrap();assert_eq!(work,committed);
    work.reopen_committed(&input,&pack.id,pack.pack_revision,"reopen").unwrap();
    let reopened=work.session(&input,&pack.id).unwrap();assert_ne!(reopened["submission_operation_id"],operation);
    assert_eq!(reopened["submission_operation_id"],work.session(&input,&pack.id).unwrap()["submission_operation_id"]);
}

#[test]
fn nested_business_rejection_is_not_success() {
    let accepted=|result:&Result<Value,String>|result.as_ref().is_ok_and(|value|value["ok"]!=false);
    assert!(!accepted(&Ok(json!({"ok":false,"feedback":{"code":"operation_conflict"}}))));
    assert!(!accepted(&Err("schema rejected".into())));
    assert!(accepted(&Ok(json!({"ok":true}))));
    assert!(accepted(&Ok(json!({"needs_review":true}))),"accepted review can still record semantic failure");
}

#[test]
fn opaque_evidence_keys_bind_current_scope_and_select_exact_grid_and_text() {
    let input = input(vec![source("text",0,"正文😀","one"),source("table-source",1,"","one")],vec![json!({"source_unit_revision_id":"table-source",
        "form_definition_revision_id":"table","definition":{"row_count":3,"column_count":1,
        "cells":[{"row":0,"column":0,"text":"列名","header_role":"column_header"},{"row":1,"column":0,"text":"内容😀","header_role":"none"},{"row":2,"column":0,"text":"","header_role":"none"}]}})]);
    let mut work=DiscoverWork::plan(&input,32768);let pack=work.claim(1).remove(0);
    let rendered=materialize_pack(&input,&pack).unwrap();let mut selected=0;
    for (atom,wire) in pack.atoms.iter().zip(rendered["atoms"].as_array().unwrap()) {
        let scope=atom_scope(atom,&pack.input_digest,true);
        for field in ["excerpts","cells","header_context"] {
            for item in wire[field].as_array().into_iter().flatten() {
                let Some(index)=item["evidence_index"].as_u64() else {assert!(item.get("evidence_key").is_none());continue};
                let key=item["evidence_key"].as_str().unwrap();assert_eq!(key.len(),27);
                let make=|key:&str|json!({"requirements":[{"source_section_id":{"atom_key":atom_key(&pack,atom).unwrap()},"evidence":[{"evidence_key":key}]}]});
                let mut body=make(key);resolve_submission_handles(&pack,&mut body).unwrap();
                assert_eq!(body["requirements"][0]["evidence"][0],json!(scope[index as usize]));
                for changed in 0..4 {let mut foreign=pack.clone();match changed {0=>foreign.id.push('x'),1=>foreign.pack_revision+=1,2=>foreign.claim_token.push('x'),_=>foreign.input_digest.push('x')};assert!(resolve_submission_handles(&foreign,&mut make(key)).is_err());}
                assert!(resolve_submission_handles(&pack,&mut make("ev_000000000000000000000000")).is_err());
                let mut mixed=make(key);mixed["requirements"][0]["evidence"][0]["atom_ref"]=json!(0);assert!(resolve_submission_handles(&pack,&mut mixed).is_err());
                selected+=1;
            }
        }
    }
    assert_eq!(selected,3,"text, header, body; blank has no key");
}

#[test]
fn condition_support_keys_are_separate_from_owned_evidence_and_revision_bound() {
    let input=input(vec![source("a",0,"主条款","one"),source("b",1,"外部条件","two")],vec![]);
    let mut work=DiscoverWork::plan(&input,32768);let mut pack=work.claim(1).remove(0);
    let external=text_ref(&input,"b",0,"外部条件".len());
    pack.atoms.retain(|atom| !atom_scope(atom,&pack.input_digest,true).contains(&external));
    let before=pack_scope(&pack,true);
    let option=ConditionSupport{source_requirement_id:"pack-0:0".into(),review_version:"checked-version".into(),evidence:vec![external.clone()]};
    pack.condition_support_options.push(option.clone());let key=support_key(&pack,&option).unwrap();
    let selected_atom=atom_key(&pack,&pack.atoms[0]).unwrap();
    let make=|key:&str|json!({"requirements":[{"source_section_id":{"atom_key":selected_atom},"evidence":[],"condition_support":[{"support_key":key}]}]});
    let mut body=make(&key);resolve_submission_handles(&pack,&mut body).unwrap();
    assert_eq!(body["requirements"][0]["condition_support"][0],json!(option));
    assert_eq!(body["requirements"][0]["evidence"],json!([]));assert_eq!(pack_scope(&pack,true),before);
    let mut forged=make("cs_forged");assert!(resolve_submission_handles(&pack,&mut forged).is_err());
    let mut main=json!({"requirements":[{"evidence":[{"evidence_key":key}]}]});assert!(resolve_submission_handles(&pack,&mut main).is_err());
    pack.pack_revision+=1;assert!(resolve_submission_handles(&pack,&mut make(&key)).is_err());
    pack.pack_revision-=1;pack.condition_support_options.clear();assert!(resolve_submission_handles(&pack,&mut make(&key)).is_err());
}
