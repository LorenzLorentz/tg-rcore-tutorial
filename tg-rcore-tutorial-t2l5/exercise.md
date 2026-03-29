# tg-rcore-tutorial-t2l5 练习说明：公平同步互斥实验

本章**基于 `tg-rcore-tutorial-ch8` 继续开发**，目标是把“同步原语能跑起来”提升到“同步原语公平、可测、可分析、可讨论 starvation”。

## 必做任务

1. 逐步补齐同步原语族：
   - `spinlock`
   - `mutex`
   - `semaphore`
   - `condvar`
   - `rwlock`
2. 为每种原语定义公平性语义：
   - 是否 FIFO
   - 是否允许 barging
   - 是否保证 bounded waiting
3. 建立统一观测指标：
   - 锁竞争次数
   - 平均持锁时间
   - 最大等待时间
   - 上下文切换次数
   - starvation 次数
4. 设计经典问题实验：
   - 生产者 - 消费者
   - 读者 - 写者
   - 哲学家进餐
5. 为每种原语准备“能失败的对照测试”思路，例如：
   - 去掉 wakeup
   - 调换 unlock 和唤醒顺序
   - condvar 用 `if` 代替 `while`
   - rwlock 不做公平队列

## 建议扩展

- ticket lock / MCS-like 公平自旋锁
- handoff mutex
- reader-prefer / writer-prefer / fair rwlock 的对比
- starvation 阈值的参数化

## 验收要求

- 正确版本能通过压力测试
- 对照错误版本能稳定暴露失败
- 能解释自旋锁与睡眠锁在高竞争下的差异
- 能说明为什么条件变量必须和谓词循环配合使用

## 说明

- 本章当前目录是从 `ch8` 复制出来的开发起点，不代表这些功能已经实现。
- 详细设计、落点和我的建议请看 [README.md](./README.md)。
