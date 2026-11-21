import { test, expect, type Page } from '@playwright/test'
import { readFile, mkdir } from 'node:fs/promises'
import path from 'node:path'
const username = 'admin', password = 'browser test password 2026'
const secret = 'mock-controller-secret-for-tests', controller = 'http://127.0.0.1:19090'
const root = path.resolve('..')
const updateScreenshots = process.env.UPDATE_SCREENSHOTS === '1'
test.describe.configure({ mode: 'serial' })
async function login(page: Page) {
  await page.goto('/')
  await page.getByLabel('用户名', { exact: true }).fill(username)
  await page.locator('input[name="password"]').fill(password)
  await page.getByRole('button', { name: '进入管理空间' }).click()
  await expect(page.locator('.main-content h1')).toHaveText('网络概览')
  await expect(page.locator('.topbar .connection-status')).toContainText('内核已连接')
}

test('first-run wizard is token protected and requires a real backend test', async ({ page }) => {
  const data = (await readFile(path.join(root, '.tools/e2e-current.txt'), 'utf8')).trim()
  const token = (await readFile(path.join(data, 'bootstrap-token'), 'utf8')).trim()
  await page.goto('/')
  await expect(page.getByRole('heading', { name: '从一个安全的账户开始' })).toBeVisible()
  await page.locator('input[name="bootstrap-token"]').fill(token)
  await page.getByLabel('用户名', { exact: true }).fill(username)
  await page.locator('input[name="password"]').fill(password)
  await page.locator('input[name="confirm-password"]').fill(password)
  await page.getByRole('button', { name: '下一步' }).click()
  await page.getByLabel('Controller 地址', { exact: true }).fill(controller)
  await page.locator('input[name="secret"]').fill(secret)
  await expect(page.getByRole('button', { name: '完成配置', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '测试连接', exact: true }).click()
  await expect(page.getByRole('button', { name: /已连接/ })).toBeVisible()
  await page.getByRole('button', { name: '完成配置', exact: true }).click()
  await expect(page.getByRole('heading', { name: '欢迎回来' })).toBeVisible()
})

test('login, real proxy selection, latency testing and secret isolation', async ({ page }) => {
  const responses: string[] = []
  page.on('response', async (response) => { if (response.url().includes('/api/') && response.request().method() === 'GET') responses.push(await response.text().catch(() => '')) })
  await login(page)
  await page.locator('.sidebar-nav').getByRole('button', { name: '代理节点' }).click()
  const group = page.locator('.proxy-group').filter({ has: page.locator('strong', { hasText: /^PROXY/ }) }).first()
  await group.getByRole('button', { name: '选择 日本 / Tokyo 02', exact: true }).click()
  await expect(group.getByRole('button', { name: '选择 日本 / Tokyo 02', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await group.getByRole('button', { name: '测试 日本 / Tokyo 02 延迟', exact: true }).click()
  await expect(group.getByRole('button', { name: '测试 日本 / Tokyo 02 延迟', exact: true })).toContainText('67 ms')
  await expect(page.locator('.proxy-group').filter({ hasText: 'URLTest' }).getByRole('button', { name: '查看 香港 · HK 01', exact: true })).toBeDisabled()
  for (const body of responses) { expect(body).not.toContain(secret); expect(body).not.toContain(controller) }
  const local = await page.evaluate(() => ({ ...localStorage }))
  expect(Object.keys(local)).toEqual(['mwp-theme'])
  if (updateScreenshots) {
    await mkdir(path.join(root, 'docs/images'), { recursive: true })
    await page.evaluate(() => window.scrollTo(0, 0))
    await expect(page.locator('.toast-stack').getByRole('button')).toHaveCount(0, { timeout: 10000 })
    await page.screenshot({ path: path.join(root, 'docs/images/nodes-desktop.png') })
    await page.locator('.sidebar-nav').getByRole('button', { name: '网络概览' }).click()
    await expect.poll(async () => (await page.locator('.chart-download').getAttribute('d'))?.split(' L ').length || 0, { timeout: 35000 }).toBeGreaterThanOrEqual(25)
    await page.screenshot({ path: path.join(root, 'docs/images/overview-desktop.png') })
  }
})

test('settings update, password rotation, sessions and honest unsupported autostart', async ({ page }) => {
  await login(page)
  await page.locator('.sidebar-nav').getByRole('button', { name: '设置', exact: true }).click()
  await page.getByLabel('原密码', { exact: true }).fill(password)
  await page.getByLabel('新密码', { exact: true }).fill('changed browser password 2026')
  await page.getByLabel('确认新密码', { exact: true }).fill('changed browser password 2026')
  await page.getByRole('button', { name: '更新账户', exact: true }).click()
  await expect(page.locator('.toast-stack')).toContainText('账户已更新')
  await page.getByLabel('原密码', { exact: true }).fill('changed browser password 2026')
  await page.getByLabel('新密码', { exact: true }).fill(password)
  await page.getByLabel('确认新密码', { exact: true }).fill(password)
  await page.getByRole('button', { name: '更新账户', exact: true }).click()
  await expect(page.getByLabel('原密码', { exact: true })).toHaveValue('')
  await page.getByRole('button', { name: '服务设置', exact: true }).click()
  await page.getByLabel('流量刷新频率', { exact: true }).selectOption('2000')
  await page.getByRole('button', { name: '保存服务设置' }).click()
  await expect(page.locator('.toast-stack')).toContainText('服务设置已保存')
  await page.getByRole('button', { name: '内核连接', exact: true }).click()
  await expect(page.getByLabel('Controller 地址', { exact: true })).toHaveValue('')
  await page.getByLabel('Controller 地址', { exact: true }).fill(controller)
  await page.getByLabel('API Secret', { exact: true }).fill(secret)
  await page.getByRole('button', { name: '测试连接', exact: true }).click()
  await expect(page.getByRole('button', { name: '保存连接配置' })).toBeEnabled()
  await page.getByRole('button', { name: '保存连接配置' }).click()
  await expect(page.getByLabel('API Secret', { exact: true })).toHaveValue('')
  await page.getByRole('button', { name: '开机自启', exact: true }).click()
  await expect(page.getByRole('heading', { name: '当前环境暂不支持此操作' })).toBeVisible()
})

test('mobile layout, themes, logs and connection confirmation', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await login(page)
  await expect(page.locator('.mobile-nav')).toBeVisible()
  await page.getByLabel('切换主题').selectOption('dark')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await page.getByLabel('切换主题').selectOption('light')
  if (updateScreenshots) {
    await expect.poll(async () => (await page.locator('.chart-download').getAttribute('d'))?.split(' L ').length || 0, { timeout: 35000 }).toBeGreaterThanOrEqual(15)
    await page.evaluate(() => window.scrollTo(0, 0))
    await page.screenshot({ path: path.join(root, 'docs/images/overview-mobile.png') })
  }
  await page.locator('.mobile-nav').getByRole('button', { name: '节点', exact: true }).click()
  await page.getByRole('searchbox', { name: '搜索节点或代理组' }).fill('Tokyo')
  await expect(page.locator('.node-copy strong').first()).toContainText('Tokyo')
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
  await page.locator('.mobile-nav').getByRole('button', { name: '连接', exact: true }).click()
  await page.getByRole('button', { name: '关闭全部', exact: true }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await page.locator('.mobile-nav').getByRole('button', { name: '日志', exact: true }).click()
  await expect(page.locator('.log-entry').first()).toBeVisible()
  await page.getByRole('button', { name: '暂停', exact: true }).click()
  await expect(page.getByRole('button', { name: '继续', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
})

test('backend offline state remains usable', async ({ page }) => {
  await page.route('**/api/status', (route) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ configured: true, connected: false, version: null, message: '无法连接 Mihomo，请检查内核状态和后端配置' }) }))
  await page.route('**/api/proxies', (route) => route.fulfill({ status: 502, contentType: 'application/json', body: JSON.stringify({ error: 'backend_unavailable', message: '无法连接 Mihomo' }) }))
  await loginOffline(page)
  await expect(page.locator('.offline-banner')).toContainText('Mihomo 暂时离线')
  await page.locator('.offline-banner').getByRole('button', { name: '检查设置' }).click()
  await expect(page.locator('.main-content h1')).toHaveText('设置')
})
async function loginOffline(page: Page) { await page.goto('/'); await page.getByLabel('用户名', { exact: true }).fill(username); await page.locator('input[name="password"]').fill(password); await page.getByRole('button', { name: '进入管理空间' }).click(); await expect(page.locator('.main-content h1')).toHaveText('网络概览') }
