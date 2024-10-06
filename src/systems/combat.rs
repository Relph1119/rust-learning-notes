use crate::prelude::*;

#[system]
#[read_component(WantsToAttack)]
#[read_component(Player)]
#[write_component(Health)]
pub fn combat(ecs: &mut SubWorld, commands: &mut CommandBuffer) {
    // 希望发起攻击的实体列表
    let mut attackers = <(Entity, &WantsToAttack)>::query();
    // 被攻击者的列表
    let victims: Vec<(Entity, Entity)> = attackers
        .iter(ecs)
        .map(|(entity, attack)| (*entity, attack.victim))
        .collect();
    victims.iter().for_each(|(message, victim)| {
        // 获取玩家角色
        let is_player = ecs.entry_ref(*victim).unwrap().get_component::<Player>().is_ok();
        // 针对只包含生命值的被攻击对象执行操作
        if let Ok(health) = ecs
            .entry_mut(*victim)
            .unwrap()
            .get_component_mut::<Health>()
        {
            println!("Health before attack: {}", health.current);
            health.current -= 1;
            // 消灭怪物
            if health.current < 1 && !is_player {
                commands.remove(*victim);
            }
            println!("Health after attack: {}", health.current);
        }
        commands.remove(*message);
    });
}