ALTER TABLE agent_runs ADD COLUMN context_used INTEGER;
ALTER TABLE agent_runs ADD COLUMN context_size INTEGER;
ALTER TABLE agent_runs ADD COLUMN context_cost_amount REAL;
ALTER TABLE agent_runs ADD COLUMN context_cost_currency TEXT;
