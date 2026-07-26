// 脚本引擎单元测试（括号式语法）

use crate::script::parser::Parser;
use crate::script::ast::*;

fn parse(src: &str) -> Result<Vec<Command>, String> {
    Parser::new(src)?.parse()
}

#[test]
fn test_basic_keyboard() {
    let cmds = parse("down(1)\nup(1)\nclick(2)\nclick_ms(a,50)\ndelay_ms(500)").unwrap();
    assert_eq!(cmds.len(), 5);
    assert_eq!(cmds[0], Command::Down("1".to_string()));
    assert_eq!(cmds[1], Command::Up("1".to_string()));
    assert_eq!(cmds[2], Command::Click("2".to_string()));
    assert_eq!(cmds[3], Command::ClickMs("a".to_string(), 50));
    assert_eq!(cmds[4], Command::DelayMs(500));
}

#[test]
fn test_comments_ignored() {
    let src = "// 注释\ndown(1) // 行尾注释\n// 又一行注释\ndelay_ms(100)";
    let cmds = parse(src).unwrap();
    assert_eq!(cmds.len(), 2);
}

#[test]
fn test_mouse_commands() {
    let cmds = parse(
        "mouse_down(left,100,200)\n\
         mouse_click(right,10,20)\n\
         mouse_up(left)\n\
         mouse_down_center(left,0,0)\n\
         mouse_click_percent(middle,50,60)"
    ).unwrap();

    assert_eq!(cmds[0], Command::MouseDown(MouseButton::Left, Coord::Absolute { x: 100, y: 200 }));
    assert_eq!(cmds[1], Command::MouseClick(MouseButton::Right, Coord::Absolute { x: 10, y: 20 }));
    assert_eq!(cmds[2], Command::MouseUp(MouseButton::Left));
    assert_eq!(cmds[3], Command::MouseDown(MouseButton::Left, Coord::Center { dx: 0, dy: 0 }));
    assert_eq!(cmds[4], Command::MouseClick(MouseButton::Middle, Coord::Percent { px: 50, py: 60 }));
}

#[test]
fn test_if_block_with_bool() {
    let src = "if_start[find_color(100,200,10,20,#ff00ff) == true]\n\
                   delay_ms(500)\n\
               if_end";
    let cmds = parse(src).unwrap();
    assert_eq!(cmds.len(), 1);

    match &cmds[0] {
        Command::If { condition, then_block, else_if_blocks } => {
            assert_eq!(condition.op, CompareOp::Eq);
            assert_eq!(condition.right, Value::Bool(true));
            assert!(matches!(condition.left, Value::FindColor { color: 0xff00ff, .. }));
            assert_eq!(then_block.len(), 1);
            assert_eq!(else_if_blocks.len(), 0);
        }
        _ => panic!("应为 If"),
    }
}

#[test]
fn test_if_else_if() {
    let src = "if_start[find_color(1,2,3,4,#ffffff) == true]\n\
                   click(1)\n\
               else_if[find_color(5,6,7,8,#000000) != true]\n\
                   click(2)\n\
               if_end";
    let cmds = parse(src).unwrap();

    match &cmds[0] {
        Command::If { then_block, else_if_blocks, .. } => {
            assert_eq!(then_block.len(), 1);
            assert_eq!(else_if_blocks.len(), 1);
            assert_eq!(else_if_blocks[0].0.op, CompareOp::Ne);
        }
        _ => panic!("应为 If"),
    }
}

#[test]
fn test_nested_if() {
    let src = "if_start[find_color(1,2,3,4,#ffffff) == true]\n\
                   if_start[find_color(5,6,7,8,#000000) == true]\n\
                       delay_ms(100)\n\
                   if_end\n\
               if_end";
    let cmds = parse(src).unwrap();

    match &cmds[0] {
        Command::If { then_block, .. } => {
            assert_eq!(then_block.len(), 1);
            assert!(matches!(then_block[0], Command::If { .. }));
        }
        _ => panic!("应为 If"),
    }
}

#[test]
fn test_compare_two_find_colors() {
    let src = "if_start[find_color(1,2,3,4,#ffffff) != find_color(5,6,7,8,#000000)]\n\
                   delay_ms(100)\n\
               if_end";
    let cmds = parse(src).unwrap();
    match &cmds[0] {
        Command::If { condition, .. } => {
            assert_eq!(condition.op, CompareOp::Ne);
            assert!(matches!(condition.left, Value::FindColor { .. }));
            assert!(matches!(condition.right, Value::FindColor { .. }));
        }
        _ => panic!("应为 If"),
    }
}

#[test]
fn test_unknown_command_errors() {
    assert!(parse("foobar(1)").is_err());
}

#[test]
fn test_if_without_end_errors() {
    let src = "if_start[find_color(1,2,3,4,#ffffff) == true]\n click(1)";
    assert!(parse(src).is_err());
}

#[test]
fn test_setting_audio_onlyonce() {
    let cmds = parse("setting(audio_onlyonce)\nclick(1)").unwrap();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0], Command::Setting(Setting::AudioOnlyOnce));
    assert_eq!(cmds[1], Command::Click("1".to_string()));
}

#[test]
fn test_unknown_setting_errors() {
    assert!(parse("setting(unknown)").is_err());
}
