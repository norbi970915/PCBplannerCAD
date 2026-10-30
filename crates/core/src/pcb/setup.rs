//! Validate editable design-rule drafts without changing a board or rejecting old files.
use std::collections::{BTreeMap,BTreeSet};
use super::{board::{BoardSettings,RuleKind},query::RuleSet};

pub fn validate(board:&BoardSettings)->Vec<String>{
 let mut errors=RuleSet::new(board).errors().to_vec();
 if board.net_classes.is_empty(){errors.push("Add a default net class.".into());}
 let mut names=BTreeSet::new();let mut nets=BTreeMap::new();
 for class in &board.net_classes{
  if class.name.trim().is_empty()||!names.insert(class.name.clone()){errors.push(format!("Net class '{}': use a non-empty, unique name.",class.name));}
  if class.track_width<=0||class.clearance<0||class.via_drill<=0||class.via_diameter<=class.via_drill||class.diff_pair_width<0||class.diff_pair_gap<0{errors.push(format!("Net class '{}': track width and via drill must be positive, via diameter must exceed drill, and clearances/pair sizes cannot be negative.",class.name));}
  for net in &class.nets{if let Some(previous)=nets.insert(net,class.name.as_str()){if previous!=class.name{errors.push(format!("Net '{net}' is assigned to both '{previous}' and '{}'.",class.name));}}}
 }
 let mut pairs=BTreeSet::new();let mut members=BTreeSet::new();
 for pair in &board.diff_pairs{
  if pair.name.trim().is_empty()||!pairs.insert(&pair.name){errors.push("Differential pairs need unique, non-empty names.".into());}
  if pair.positive.trim().is_empty()||pair.negative.trim().is_empty()||pair.positive==pair.negative{errors.push(format!("Pair '{}': choose two different, non-empty net names.",pair.name));}
  for net in [&pair.positive,&pair.negative]{if !net.is_empty()&&!members.insert(net){errors.push(format!("Net '{net}' belongs to more than one differential-pair member."));}}
 }
 for rule in board.rules.iter().filter(|r|r.enabled){
  let valid=match &rule.kind{
   RuleKind::Width{min,preferred,max,..}=>*min>0&&min<=preferred&&preferred<=max,
   RuleKind::ViaStyle{diameter,drill,min_annular_ring,..}=>*drill>0&&diameter>drill&&*min_annular_ring>=0&&(*diameter as i128-*drill as i128)>=2*(*min_annular_ring as i128),
   RuleKind::HoleSize{min,max,..}=>*min>0&&min<=max,
   RuleKind::Length{min,max,..}=>*min>=0&&min<=max,
   RuleKind::Clearance{min,..}|RuleKind::BoardOutlineClearance{min}|RuleKind::ComponentClearance{min}|RuleKind::SilkClearance{min}=>*min>=0,
   RuleKind::DiffPair{width,gap,..}=>*width>0&&*gap>=0,
   RuleKind::ZoneConnect{gap,spoke_width,..}=>*gap>=0&&*spoke_width>0,
   RuleKind::SolderMaskExpansion{..}|RuleKind::PasteMaskExpansion{..}=>true,
  };
  if rule.name.trim().is_empty(){errors.push("Enabled rules need a name.".into());}
  if !valid{errors.push(format!("Rule '{}': invalid dimensions. Check minimum/preferred/maximum order, positive sizes and via annular ring.",rule.name));}
 }
 errors
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn defaults_are_valid_and_invalid_scopes_are_reported_without_mutation(){let mut board=BoardSettings::default();assert!(validate(&board).is_empty());board.rules[0].kind=RuleKind::Clearance{first:"InNet(".into(),second:"All".into(),min:200_000};let original=board.clone();assert!(validate(&board).iter().any(|e|e.contains(&board.rules[0].name)));assert_eq!(board,original);}
 #[test]fn reversed_width_and_impossible_via_ring_are_rejected(){let mut board=BoardSettings::default();board.rules[0].kind=RuleKind::Width{scope:"All".into(),min:300_000,preferred:200_000,max:100_000};assert!(!validate(&board).is_empty());board.rules[0].kind=RuleKind::ViaStyle{scope:"All".into(),diameter:600_000,drill:400_000,min_annular_ring:150_000};assert!(!validate(&board).is_empty());}
 #[test]fn ambiguous_classes_and_pair_members_are_rejected(){let mut board=BoardSettings::default();let mut other=board.net_classes[0].clone();other.name="Power".into();other.nets=vec!["VCC".into()];board.net_classes[0].nets=vec!["VCC".into()];board.net_classes.push(other);board.diff_pairs.push(super::super::board::DiffPair{name:"Data".into(),positive:"D".into(),negative:"D".into()});assert!(validate(&board).iter().any(|e|e.contains("assigned to both")));assert!(validate(&board).iter().any(|e|e.contains("two different")));}
}

#[derive(serde::Serialize)]
#[serde(rename_all="camelCase")]
pub struct RoutingDefaults { pub width:i64, pub pair_width:i64, pub pair_gap:i64 }
pub fn routing_defaults(board:&BoardSettings,net:&str,layer:crate::document::LayerId)->RoutingDefaults{
 let rules=RuleSet::new(board);let object=super::query::Obj::new(super::query::ObjKind::Track,net,Some(layer));let pair=rules.diff_pair(&object);
 RoutingDefaults {width:rules.width(&object).1,pair_width:pair.0,pair_gap:pair.1}
}
#[cfg(test)]mod routing_tests{
 use super::*;
 #[test]fn routing_preferences_obey_the_same_first_matching_rule_as_drc(){
  let mut board=BoardSettings::default();let mut rule=board.rules[0].clone();rule.name="Clock width".into();rule.kind=RuleKind::Width{scope:"InNet('CLOCK')".into(),min:100_000,preferred:180_000,max:300_000};board.rules.insert(0,rule);
  assert_eq!(routing_defaults(&board,"CLOCK",crate::document::LayerId(0)).width,180_000);assert_eq!(routing_defaults(&board,"OTHER",crate::document::LayerId(0)).width,250_000);board.rules[0].enabled=false;assert_eq!(routing_defaults(&board,"CLOCK",crate::document::LayerId(0)).width,250_000);
 }
 #[test]fn pair_width_and_gap_follow_scoped_rules(){let mut board=BoardSettings::default();let mut rule=board.rules[0].clone();rule.kind=RuleKind::DiffPair{scope:"InNet('DATA_P')".into(),width:140_000,gap:160_000};board.rules.insert(0,rule);let prefs=routing_defaults(&board,"DATA_P",crate::document::LayerId(0));assert_eq!((prefs.pair_width,prefs.pair_gap),(140_000,160_000));}
}
