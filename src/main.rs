mod map;
mod map_builder;
mod camera;
mod components;
mod spawner;
mod systems;

mod prelude {
    pub use bracket_lib::prelude::*;
    pub use legion::*;
    pub use legion::world::SubWorld;
    pub use legion::systems::CommandBuffer;

    pub const SCREEN_WIDTH: i32 = 80;
    pub const SCREEN_HEIGHT: i32 = 50;

    pub const DISPLAY_WIDTH: i32 = SCREEN_WIDTH / 2;
    pub const DISPLAY_HEIGHT: i32 = SCREEN_HEIGHT / 2;

    pub use crate::map::*;

    pub use crate::map_builder::*;

    pub use crate::camera::*;

    pub use crate::components::*;

    pub use crate::spawner::*;

    pub use crate::systems::*;
}

use prelude::*;

struct State {
    // 存储所有的实体和组件，Entity Component System实体组件系统
    ecs: World,
    resources: Resources,
    systems: Schedule
}

impl State {
    fn new() -> Self {
        let mut ecs = World::default();
        let mut resources = Resources::default();
        let mut rng = RandomNumberGenerator::new();
        let map_builder = MapBuilder::new(&mut rng);
        spawn_player(&mut ecs, map_builder.player_start);
        // 除第1个房间外，每个房间的中心放置一个怪物
        map_builder.rooms.iter().skip(1).map(|r| r.center())
            .for_each(|pos| spawn_monster(&mut ecs, &mut rng, pos));
        resources.insert(map_builder.map);
        resources.insert(Camera::new(map_builder.player_start));
        Self {
            ecs,
            resources,
            systems: build_scheduler()
        }
    }
}

impl GameState for State {
    fn tick(&mut self, ctx: &mut BTerm) {
        // 清空每一个图层
        ctx.set_active_console(0);
        ctx.cls();
        ctx.set_active_console(1);
        ctx.cls();
        // 将键盘的输入状态作为一个资源加入到资源列表中
        self.resources.insert(ctx.key);
        // 执行各个系统的执行计划
        self.systems.execute(&mut self.ecs, &mut self.resources);
        // 批量渲染
        render_draw_buffer(ctx).expect("Render error");
    }
}

fn main() -> BError {
    /* with_dimensions：添加控制台尺寸
     * with_tile_dimensions：设置图块的尺寸
     * with_resource_path：设置资源存放目录
     * with_font：设置加载的字体文件和尺寸
     * with_simple_console：添加一个新图层，用于绘制地图
     * with_simple_console_no_bg：添加一个透明图层，用于绘制玩家角色
     */
    let context = BTermBuilder::new()
        .with_title("Dungeon Crawler")
        .with_fps_cap(30.0)
        .with_dimensions(DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .with_tile_dimensions(32, 32)
        .with_resource_path("resources/")
        .with_font("dungeonfont.png", 32, 32)
        .with_simple_console(DISPLAY_WIDTH, DISPLAY_HEIGHT, "dungeonfont.png")
        .with_simple_console_no_bg(DISPLAY_WIDTH, DISPLAY_HEIGHT, "dungeonfont.png")
        .build()?;
    main_loop(context, State::new())
}
