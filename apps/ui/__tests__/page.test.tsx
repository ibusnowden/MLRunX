import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
}))

vi.mock('@/lib/api', () => ({
  api: {
    listRuns: vi.fn().mockResolvedValue({ runs: [], total: 0 }),
  },
}))

import Page from '../src/app/page'

describe('Home Page', () => {
  it('renders the MLRunX heading', async () => {
    render(<Page />)
    const heading = screen.getByRole('heading', { level: 1 })
    expect(heading).toBeDefined()
    expect(heading.textContent).toContain('MLRunX Experiments')
    const emptyStates = await screen.findAllByText(/no runs found/i)
    expect(emptyStates.length).toBeGreaterThan(0)
  })

  it('displays the welcome message', async () => {
    render(<Page />)
    const text = screen.getByText(/experiment tracking for ml runs anywhere/i)
    expect(text).toBeDefined()
    const emptyStates = await screen.findAllByText(/no runs found/i)
    expect(emptyStates.length).toBeGreaterThan(0)
  })
})
