Feature: What a bullet does where it lands

  Background:
    Given I am in test room 1
    And I have the smg
    And the shot sound is loaded

  Scenario: A bullet that lands on the ground leaves a hole there
    Given the target dummy is out of the way
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then a bullet hole appears in the ground

  Scenario: It throws up dirt and dust, and they settle again
    Given the target dummy is out of the way
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then dirt is thrown up
    And a puff of dust rises
    And the debris is gone again

  Scenario: And it thumps
    Given the target dummy is out of the way
    And I press Z
    And I am prone
    And I right click
    And the gun is on its sights
    When I fire once
    Then the impact thumps

  Scenario: A bullet that hits the target leaves a hole in it
    Given I right click
    And the gun is on its sights
    When I fire once
    Then a bullet hole appears on the target

  Scenario: and throws splinters and thumps
    Given I right click
    And the gun is on its sights
    When I fire once
    Then dirt is thrown up
    And the impact thumps
