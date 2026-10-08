Feature: Bullets hit the solid things in the world

  Background:
    Given I am in test room 1
    And I have the smg
    And the shot sound is loaded

  Scenario: Solid impact: a bullet is stopped by a wall and leaves a hole in it
    Given there is a 3 metre high wall ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then a bullet hole appears in the stone
    And the bullet landed 0.3 metres short of the middle line
    And nothing was hit beyond it

  Scenario: Solid impact: and chips fly, and it thumps
    Given there is a 3 metre high wall ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then dirt is thrown up
    And the impact thumps

  Scenario: Solid impact: a low wall doesn't stop a shot from standing height
    Given there is a 0.5 metre high wall ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken 25 damage

  Scenario: Solid impact: a tree trunk takes a bullet
    Given there is a tree trunk ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then a bullet hole appears in the wood

  Scenario: Foliage: a bullet goes through a hedge, slowed to a fraction of its damage
    Given there is a 1.5 metre high hedge ahead
    And I press C
    And I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken between 1 and 10 damage

  Scenario: Foliage: it bursts into leaves where it goes in
    Given there is a 1.5 metre high hedge ahead
    And I press C
    And I right click
    And the gun is on its sights
    When I fire once
    Then dirt is thrown up

  Scenario: Foliage: over the top of a hedge there is nothing to slow it
    Given there is a 1.5 metre high hedge ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken 25 damage

  Scenario: Foliage: a thin tree crown slows a bullet
    Given there is a thin tree crown ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken between 1 and 10 damage

  Scenario: Foliage: a thick tree crown stops it
    Given there is a thick tree crown ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy is unhurt

  Scenario: Solid impact: a shed is metal
    Given there is a metal shed ahead
    And I right click
    And the gun is on its sights
    When I fire once
    Then a bullet hole appears in the metal
    And the bullet landed on the shed
